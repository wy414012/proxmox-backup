//! Tools and utilities
//!
//! This is a collection of small and useful tools.
use std::any::Any;
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::io::Read;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::path::Path;
use std::time::Duration;

use failure::*;
use serde_json::Value;

use proxmox::tools::vec;

pub mod acl;
pub mod async_io;
pub mod async_mutex;
pub mod borrow;
pub mod daemon;
pub mod fs;
pub mod futures;
pub mod runtime;
pub mod ticket;
pub mod timer;
pub mod tty;
pub mod wrapped_reader_stream;
pub mod xattr;

mod process_locker;
pub use process_locker::*;

mod file_logger;
pub use file_logger::*;

mod broadcast_future;
pub use broadcast_future::*;

/// The `BufferedRead` trait provides a single function
/// `buffered_read`. It returns a reference to an internal buffer. The
/// purpose of this traid is to avoid unnecessary data copies.
pub trait BufferedRead {
    /// This functions tries to fill the internal buffers, then
    /// returns a reference to the available data. It returns an empty
    /// buffer if `offset` points to the end of the file.
    fn buffered_read(&mut self, offset: u64) -> Result<&[u8], Error>;
}

/// Directly map a type into a binary buffer. This is mostly useful
/// for reading structured data from a byte stream (file). You need to
/// make sure that the buffer location does not change, so please
/// avoid vec resize while you use such map.
///
/// This function panics if the buffer is not large enough.
pub fn map_struct<T>(buffer: &[u8]) -> Result<&T, Error> {
    if buffer.len() < ::std::mem::size_of::<T>() {
        bail!("unable to map struct - buffer too small");
    }
    Ok(unsafe { &*(buffer.as_ptr() as *const T) })
}

/// Directly map a type into a mutable binary buffer. This is mostly
/// useful for writing structured data into a byte stream (file). You
/// need to make sure that the buffer location does not change, so
/// please avoid vec resize while you use such map.
///
/// This function panics if the buffer is not large enough.
pub fn map_struct_mut<T>(buffer: &mut [u8]) -> Result<&mut T, Error> {
    if buffer.len() < ::std::mem::size_of::<T>() {
        bail!("unable to map struct - buffer too small");
    }
    Ok(unsafe { &mut *(buffer.as_ptr() as *mut T) })
}

/// Create a file lock using fntl. This function allows you to specify
/// a timeout if you want to avoid infinite blocking.
pub fn lock_file<F: AsRawFd>(
    file: &mut F,
    exclusive: bool,
    timeout: Option<Duration>,
) -> Result<(), Error> {
    let lockarg = if exclusive {
        nix::fcntl::FlockArg::LockExclusive
    } else {
        nix::fcntl::FlockArg::LockShared
    };

    let timeout = match timeout {
        None => {
            nix::fcntl::flock(file.as_raw_fd(), lockarg)?;
            return Ok(());
        }
        Some(t) => t,
    };

    // unblock the timeout signal temporarily
    let _sigblock_guard = timer::unblock_timeout_signal();

    // setup a timeout timer
    let mut timer = timer::Timer::create(
        timer::Clock::Realtime,
        timer::TimerEvent::ThisThreadSignal(timer::SIGTIMEOUT),
    )?;

    timer.arm(
        timer::TimerSpec::new()
            .value(Some(timeout))
            .interval(Some(Duration::from_millis(10))),
    )?;

    nix::fcntl::flock(file.as_raw_fd(), lockarg)?;
    Ok(())
}

/// Open or create a lock file (append mode). Then try to
/// aquire a lock using `lock_file()`.
pub fn open_file_locked<P: AsRef<Path>>(path: P, timeout: Duration) -> Result<File, Error> {
    let path = path.as_ref();
    let mut file = match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => file,
        Err(err) => bail!("Unable to open lock {:?} - {}", path, err),
    };
    match lock_file(&mut file, true, Some(timeout)) {
        Ok(_) => Ok(file),
        Err(err) => bail!("Unable to aquire lock {:?} - {}", path, err),
    }
}

/// Split a file into equal sized chunks. The last chunk may be
/// smaller. Note: We cannot implement an `Iterator`, because iterators
/// cannot return a borrowed buffer ref (we want zero-copy)
pub fn file_chunker<C, R>(mut file: R, chunk_size: usize, mut chunk_cb: C) -> Result<(), Error>
where
    C: FnMut(usize, &[u8]) -> Result<bool, Error>,
    R: Read,
{
    const READ_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4M

    if chunk_size > READ_BUFFER_SIZE {
        bail!("chunk size too large!");
    }

    let mut buf = vec::undefined(READ_BUFFER_SIZE);

    let mut pos = 0;
    let mut file_pos = 0;
    loop {
        let mut eof = false;
        let mut tmp = &mut buf[..];
        // try to read large portions, at least chunk_size
        while pos < chunk_size {
            match file.read(tmp) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(n) => {
                    pos += n;
                    if pos > chunk_size {
                        break;
                    }
                    tmp = &mut tmp[n..];
                }
                Err(ref e) if e.kind() == ErrorKind::Interrupted => { /* try again */ }
                Err(e) => bail!("read chunk failed - {}", e.to_string()),
            }
        }
        let mut start = 0;
        while start + chunk_size <= pos {
            if !(chunk_cb)(file_pos, &buf[start..start + chunk_size])? {
                break;
            }
            file_pos += chunk_size;
            start += chunk_size;
        }
        if eof {
            if start < pos {
                (chunk_cb)(file_pos, &buf[start..pos])?;
                //file_pos += pos - start;
            }
            break;
        } else {
            let rest = pos - start;
            if rest > 0 {
                let ptr = buf.as_mut_ptr();
                unsafe {
                    std::ptr::copy_nonoverlapping(ptr.add(start), ptr, rest);
                }
                pos = rest;
            } else {
                pos = 0;
            }
        }
    }

    Ok(())
}

/// Returns the Unix uid/gid for the sepcified system user.
pub fn getpwnam_ugid(username: &str) -> Result<(libc::uid_t, libc::gid_t), Error> {
    let c_username = std::ffi::CString::new(username).unwrap();
    let info = unsafe { libc::getpwnam(c_username.as_ptr()) };
    if info.is_null() {
        bail!("getwpnam '{}' failed", username);
    }

    let info = unsafe { *info };

    Ok((info.pw_uid, info.pw_gid))
}

pub fn json_object_to_query(data: Value) -> Result<String, Error> {
    let mut query = url::form_urlencoded::Serializer::new(String::new());

    let object = data.as_object().ok_or_else(|| {
        format_err!("json_object_to_query: got wrong data type (expected object).")
    })?;

    for (key, value) in object {
        match value {
            Value::Bool(b) => {
                query.append_pair(key, &b.to_string());
            }
            Value::Number(n) => {
                query.append_pair(key, &n.to_string());
            }
            Value::String(s) => {
                query.append_pair(key, &s);
            }
            Value::Array(arr) => {
                for element in arr {
                    match element {
                        Value::Bool(b) => {
                            query.append_pair(key, &b.to_string());
                        }
                        Value::Number(n) => {
                            query.append_pair(key, &n.to_string());
                        }
                        Value::String(s) => {
                            query.append_pair(key, &s);
                        }
                        _ => bail!(
                            "json_object_to_query: unable to handle complex array data types."
                        ),
                    }
                }
            }
            _ => bail!("json_object_to_query: unable to handle complex data types."),
        }
    }

    Ok(query.finish())
}

pub fn required_string_param<'a>(param: &'a Value, name: &str) -> Result<&'a str, Error> {
    match param[name].as_str() {
        Some(s) => Ok(s),
        None => bail!("missing parameter '{}'", name),
    }
}

pub fn required_string_property<'a>(param: &'a Value, name: &str) -> Result<&'a str, Error> {
    match param[name].as_str() {
        Some(s) => Ok(s),
        None => bail!("missing property '{}'", name),
    }
}

pub fn required_integer_param<'a>(param: &'a Value, name: &str) -> Result<i64, Error> {
    match param[name].as_i64() {
        Some(s) => Ok(s),
        None => bail!("missing parameter '{}'", name),
    }
}

pub fn required_integer_property<'a>(param: &'a Value, name: &str) -> Result<i64, Error> {
    match param[name].as_i64() {
        Some(s) => Ok(s),
        None => bail!("missing property '{}'", name),
    }
}

pub fn required_array_param<'a>(param: &'a Value, name: &str) -> Result<Vec<Value>, Error> {
    match param[name].as_array() {
        Some(s) => Ok(s.to_vec()),
        None => bail!("missing parameter '{}'", name),
    }
}

pub fn required_array_property<'a>(param: &'a Value, name: &str) -> Result<Vec<Value>, Error> {
    match param[name].as_array() {
        Some(s) => Ok(s.to_vec()),
        None => bail!("missing property '{}'", name),
    }
}

pub fn complete_file_name<S: BuildHasher>(arg: &str, _param: &HashMap<String, String, S>) -> Vec<String> {
    let mut result = vec![];

    use nix::fcntl::AtFlags;
    use nix::fcntl::OFlag;
    use nix::sys::stat::Mode;

    let mut dirname = std::path::PathBuf::from(if arg.is_empty() { "./" } else { arg });

    let is_dir = match nix::sys::stat::fstatat(libc::AT_FDCWD, &dirname, AtFlags::empty()) {
        Ok(stat) => (stat.st_mode & libc::S_IFMT) == libc::S_IFDIR,
        Err(_) => false,
    };

    if !is_dir {
        if let Some(parent) = dirname.parent() {
            dirname = parent.to_owned();
        }
    }

    let mut dir =
        match nix::dir::Dir::openat(libc::AT_FDCWD, &dirname, OFlag::O_DIRECTORY, Mode::empty()) {
            Ok(d) => d,
            Err(_) => return result,
        };

    for item in dir.iter() {
        if let Ok(entry) = item {
            if let Ok(name) = entry.file_name().to_str() {
                if name == "." || name == ".." {
                    continue;
                }
                let mut newpath = dirname.clone();
                newpath.push(name);

                if let Ok(stat) =
                    nix::sys::stat::fstatat(libc::AT_FDCWD, &newpath, AtFlags::empty())
                {
                    if (stat.st_mode & libc::S_IFMT) == libc::S_IFDIR {
                        newpath.push("");
                        if let Some(newpath) = newpath.to_str() {
                            result.push(newpath.to_owned());
                        }
                        continue;
                    }
                }
                if let Some(newpath) = newpath.to_str() {
                    result.push(newpath.to_owned());
                }
            }
        }
    }

    result
}

/// Scan directory for matching file names.
///
/// Scan through all directory entries and call `callback()` function
/// if the entry name matches the regular expression. This function
/// used unix `openat()`, so you can pass absolute or relative file
/// names. This function simply skips non-UTF8 encoded names.
pub fn scandir<P, F>(
    dirfd: RawFd,
    path: &P,
    regex: &regex::Regex,
    mut callback: F,
) -> Result<(), Error>
where
    F: FnMut(RawFd, &str, nix::dir::Type) -> Result<(), Error>,
    P: ?Sized + nix::NixPath,
{
    for entry in self::fs::scan_subdir(dirfd, path, regex)? {
        let entry = entry?;
        let file_type = match entry.file_type() {
            Some(file_type) => file_type,
            None => bail!("unable to detect file type"),
        };

        callback(
            entry.parent_fd(),
            unsafe { entry.file_name_utf8_unchecked() },
            file_type,
        )?;
    }
    Ok(())
}

pub fn get_hardware_address() -> Result<String, Error> {
    static FILENAME: &str = "/etc/ssh/ssh_host_rsa_key.pub";

    let contents = proxmox::tools::fs::file_get_contents(FILENAME)?;
    let digest = md5::compute(contents);

    Ok(format!("{:0x}", digest))
}

pub fn assert_if_modified(digest1: &str, digest2: &str) -> Result<(), Error> {
    if digest1 != digest2 {
        bail!("detected modified configuration - file changed by other user? Try again.");
    }
    Ok(())
}

/// Extract authentication cookie from cookie header.
/// We assume cookie_name is already url encoded.
pub fn extract_auth_cookie(cookie: &str, cookie_name: &str) -> Option<String> {
    for pair in cookie.split(';') {
        let (name, value) = match pair.find('=') {
            Some(i) => (pair[..i].trim(), pair[(i + 1)..].trim()),
            None => return None, // Cookie format error
        };

        if name == cookie_name {
            use url::percent_encoding::percent_decode;
            if let Ok(value) = percent_decode(value.as_bytes()).decode_utf8() {
                return Some(value.into());
            } else {
                return None; // Cookie format error
            }
        }
    }

    None
}

pub fn join(data: &Vec<String>, sep: char) -> String {
    let mut list = String::new();

    for item in data {
        if !list.is_empty() {
            list.push(sep);
        }
        list.push_str(item);
    }

    list
}

/// normalize uri path
///
/// Do not allow ".", "..", or hidden files ".XXXX"
/// Also remove empty path components
pub fn normalize_uri_path(path: &str) -> Result<(String, Vec<&str>), Error> {
    let items = path.split('/');

    let mut path = String::new();
    let mut components = vec![];

    for name in items {
        if name.is_empty() {
            continue;
        }
        if name.starts_with('.') {
            bail!("Path contains illegal components.");
        }
        path.push('/');
        path.push_str(name);
        components.push(name);
    }

    Ok((path, components))
}

pub fn fd_change_cloexec(fd: RawFd, on: bool) -> Result<(), Error> {
    use nix::fcntl::{fcntl, FdFlag, F_GETFD, F_SETFD};
    let mut flags = FdFlag::from_bits(fcntl(fd, F_GETFD)?)
        .ok_or_else(|| format_err!("unhandled file flags"))?; // nix crate is stupid this way...
    flags.set(FdFlag::FD_CLOEXEC, on);
    fcntl(fd, F_SETFD(flags))?;
    Ok(())
}

static mut SHUTDOWN_REQUESTED: bool = false;

pub fn request_shutdown() {
    unsafe {
        SHUTDOWN_REQUESTED = true;
    }
    crate::server::server_shutdown();
}

#[inline(always)]
pub fn shutdown_requested() -> bool {
    unsafe { SHUTDOWN_REQUESTED }
}

pub fn fail_on_shutdown() -> Result<(), Error> {
    if shutdown_requested() {
        bail!("Server shutdown requested - aborting task");
    }
    Ok(())
}

/// Guard a raw file descriptor with a drop handler. This is mostly useful when access to an owned
/// `RawFd` is required without the corresponding handler object (such as when only the file
/// descriptor number is required in a closure which may be dropped instead of being executed).
pub struct Fd(pub RawFd);

impl Drop for Fd {
    fn drop(&mut self) {
        if self.0 != -1 {
            unsafe {
                libc::close(self.0);
            }
        }
    }
}

impl AsRawFd for Fd {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

impl IntoRawFd for Fd {
    fn into_raw_fd(mut self) -> RawFd {
        let fd = self.0;
        self.0 = -1;
        fd
    }
}

impl FromRawFd for Fd {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self(fd)
    }
}

// wrap nix::unistd::pipe2 + O_CLOEXEC into something returning guarded file descriptors
pub fn pipe() -> Result<(Fd, Fd), Error> {
    let (pin, pout) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC)?;
    Ok((Fd(pin), Fd(pout)))
}

/// An easy way to convert types to Any
///
/// Mostly useful to downcast trait objects (see RpcEnvironment).
pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
