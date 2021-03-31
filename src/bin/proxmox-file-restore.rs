use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, format_err, Error};
use serde_json::Value;

use proxmox::api::{
    api,
    cli::{run_cli_command, CliCommand, CliCommandMap, CliEnvironment},
};
use pxar::accessor::aio::Accessor;

use proxmox_backup::api2::{helpers, types::ArchiveEntry};
use proxmox_backup::backup::{
    decrypt_key, BackupDir, BufferedDynamicReader, CatalogReader, CryptConfig, CryptMode,
    DirEntryAttribute, IndexFile, LocalDynamicReadAt, CATALOG_NAME,
};
use proxmox_backup::client::{BackupReader, RemoteChunkReader};
use proxmox_backup::pxar::{create_zip, extract_sub_dir};
use proxmox_backup::tools;

// use "pub" so rust doesn't complain about "unused" functions in the module
pub mod proxmox_client_tools;
use proxmox_client_tools::{
    complete_group_or_snapshot, complete_repository, connect, extract_repository_from_value,
    key_source::{
        crypto_parameters, format_key_source, get_encryption_key_password, KEYFD_SCHEMA,
        KEYFILE_SCHEMA,
    },
    REPO_URL_SCHEMA,
};

enum ExtractPath {
    ListArchives,
    Pxar(String, Vec<u8>),
}

fn parse_path(path: String, base64: bool) -> Result<ExtractPath, Error> {
    let mut bytes = if base64 {
        base64::decode(path)?
    } else {
        path.into_bytes()
    };

    if bytes == b"/" {
        return Ok(ExtractPath::ListArchives);
    }

    while bytes.len() > 0 && bytes[0] == b'/' {
        bytes.remove(0);
    }

    let (file, path) = {
        let slash_pos = bytes.iter().position(|c| *c == b'/').unwrap_or(bytes.len());
        let path = bytes.split_off(slash_pos);
        let file = String::from_utf8(bytes)?;
        (file, path)
    };

    if file.ends_with(".pxar.didx") {
        Ok(ExtractPath::Pxar(file, path))
    } else {
        bail!("'{}' is not supported for file-restore", file);
    }
}

#[api(
   input: {
       properties: {
           repository: {
               schema: REPO_URL_SCHEMA,
               optional: true,
           },
           snapshot: {
               type: String,
               description: "Group/Snapshot path.",
           },
           "path": {
               description: "Path to restore. Directories will be restored as .zip files.",
               type: String,
           },
           "base64": {
               type: Boolean,
               description: "If set, 'path' will be interpreted as base64 encoded.",
               optional: true,
               default: false,
           },
           keyfile: {
               schema: KEYFILE_SCHEMA,
               optional: true,
           },
           "keyfd": {
               schema: KEYFD_SCHEMA,
               optional: true,
           },
           "crypt-mode": {
               type: CryptMode,
               optional: true,
           },
       }
   }
)]
/// List a directory from a backup snapshot.
async fn list(
    snapshot: String,
    path: String,
    base64: bool,
    param: Value,
) -> Result<Vec<ArchiveEntry>, Error> {
    let repo = extract_repository_from_value(&param)?;
    let snapshot: BackupDir = snapshot.parse()?;
    let path = parse_path(path, base64)?;

    let crypto = crypto_parameters(&param)?;
    let crypt_config = match crypto.enc_key {
        None => None,
        Some(ref key) => {
            let (key, _, _) =
                decrypt_key(&key.key, &get_encryption_key_password).map_err(|err| {
                    eprintln!("{}", format_key_source(&key.source, "encryption"));
                    err
                })?;
            Some(Arc::new(CryptConfig::new(key)?))
        }
    };

    let client = connect(&repo)?;
    let client = BackupReader::start(
        client,
        crypt_config.clone(),
        repo.store(),
        &snapshot.group().backup_type(),
        &snapshot.group().backup_id(),
        snapshot.backup_time(),
        true,
    )
    .await?;

    let (manifest, _) = client.download_manifest().await?;
    manifest.check_fingerprint(crypt_config.as_ref().map(Arc::as_ref))?;

    match path {
        ExtractPath::ListArchives => {
            let mut entries = vec![];
            for file in manifest.files() {
                match file.filename.rsplitn(2, '.').next().unwrap() {
                    "didx" => {}
                    "fidx" => {}
                    _ => continue, // ignore all non fidx/didx
                }
                let path = format!("/{}", file.filename);
                let attr = DirEntryAttribute::Directory { start: 0 };
                entries.push(ArchiveEntry::new(path.as_bytes(), &attr));
            }

            Ok(entries)
        }
        ExtractPath::Pxar(file, mut path) => {
            let index = client
                .download_dynamic_index(&manifest, CATALOG_NAME)
                .await?;
            let most_used = index.find_most_used_chunks(8);
            let file_info = manifest.lookup_file_info(&CATALOG_NAME)?;
            let chunk_reader = RemoteChunkReader::new(
                client.clone(),
                crypt_config,
                file_info.chunk_crypt_mode(),
                most_used,
            );
            let reader = BufferedDynamicReader::new(index, chunk_reader);
            let mut catalog_reader = CatalogReader::new(reader);

            let mut fullpath = file.into_bytes();
            fullpath.append(&mut path);

            helpers::list_dir_content(&mut catalog_reader, &fullpath)
        }
    }
}

#[api(
   input: {
       properties: {
           repository: {
               schema: REPO_URL_SCHEMA,
               optional: true,
           },
           snapshot: {
               type: String,
               description: "Group/Snapshot path.",
           },
           "path": {
               description: "Path to restore. Directories will be restored as .zip files if extracted to stdout.",
               type: String,
           },
           "base64": {
               type: Boolean,
               description: "If set, 'path' will be interpreted as base64 encoded.",
               optional: true,
               default: false,
           },
           target: {
               type: String,
               optional: true,
               description: "Target directory path. Use '-' to write to standard output.",
           },
           keyfile: {
               schema: KEYFILE_SCHEMA,
               optional: true,
           },
           "keyfd": {
               schema: KEYFD_SCHEMA,
               optional: true,
           },
           "crypt-mode": {
               type: CryptMode,
               optional: true,
           },
           verbose: {
               type: Boolean,
               description: "Print verbose information",
               optional: true,
               default: false,
           }
       }
   }
)]
/// Restore files from a backup snapshot.
async fn extract(
    snapshot: String,
    path: String,
    base64: bool,
    target: Option<String>,
    verbose: bool,
    param: Value,
) -> Result<(), Error> {
    let repo = extract_repository_from_value(&param)?;
    let snapshot: BackupDir = snapshot.parse()?;
    let orig_path = path;
    let path = parse_path(orig_path.clone(), base64)?;

    let target = match target {
        Some(target) if target == "-" => None,
        Some(target) => Some(PathBuf::from(target)),
        None => Some(std::env::current_dir()?),
    };

    let crypto = crypto_parameters(&param)?;
    let crypt_config = match crypto.enc_key {
        None => None,
        Some(ref key) => {
            let (key, _, _) =
                decrypt_key(&key.key, &get_encryption_key_password).map_err(|err| {
                    eprintln!("{}", format_key_source(&key.source, "encryption"));
                    err
                })?;
            Some(Arc::new(CryptConfig::new(key)?))
        }
    };

    match path {
        ExtractPath::Pxar(archive_name, path) => {
            let client = connect(&repo)?;
            let client = BackupReader::start(
                client,
                crypt_config.clone(),
                repo.store(),
                &snapshot.group().backup_type(),
                &snapshot.group().backup_id(),
                snapshot.backup_time(),
                true,
            )
            .await?;
            let (manifest, _) = client.download_manifest().await?;
            let file_info = manifest.lookup_file_info(&archive_name)?;
            let index = client
                .download_dynamic_index(&manifest, &archive_name)
                .await?;
            let most_used = index.find_most_used_chunks(8);
            let chunk_reader = RemoteChunkReader::new(
                client.clone(),
                crypt_config,
                file_info.chunk_crypt_mode(),
                most_used,
            );
            let reader = BufferedDynamicReader::new(index, chunk_reader);

            let archive_size = reader.archive_size();
            let reader = LocalDynamicReadAt::new(reader);
            let decoder = Accessor::new(reader, archive_size).await?;

            let root = decoder.open_root().await?;
            let file = root
                .lookup(OsStr::from_bytes(&path))
                .await?
                .ok_or(format_err!("error opening '{:?}'", path))?;

            if let Some(target) = target {
                extract_sub_dir(target, decoder, OsStr::from_bytes(&path), verbose).await?;
            } else {
                match file.kind() {
                    pxar::EntryKind::File { .. } => {
                        tokio::io::copy(&mut file.contents().await?, &mut tokio::io::stdout())
                            .await?;
                    }
                    _ => {
                        create_zip(
                            tokio::io::stdout(),
                            decoder,
                            OsStr::from_bytes(&path),
                            verbose,
                        )
                        .await?;
                    }
                }
            }
        }
        _ => {
            bail!("cannot extract '{}'", orig_path);
        }
    }

    Ok(())
}

fn main() {
    let list_cmd_def = CliCommand::new(&API_METHOD_LIST)
        .arg_param(&["snapshot", "path"])
        .completion_cb("repository", complete_repository)
        .completion_cb("snapshot", complete_group_or_snapshot);

    let restore_cmd_def = CliCommand::new(&API_METHOD_EXTRACT)
        .arg_param(&["snapshot", "path", "target"])
        .completion_cb("repository", complete_repository)
        .completion_cb("snapshot", complete_group_or_snapshot)
        .completion_cb("target", tools::complete_file_name);

    let cmd_def = CliCommandMap::new()
        .insert("list", list_cmd_def)
        .insert("extract", restore_cmd_def);

    let rpcenv = CliEnvironment::new();
    run_cli_command(
        cmd_def,
        rpcenv,
        Some(|future| proxmox_backup::tools::runtime::main(future)),
    );
}
