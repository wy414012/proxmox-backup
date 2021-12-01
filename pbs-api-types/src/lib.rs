//! Basic API types used by most of the PBS code.

use serde::{Deserialize, Serialize};
use anyhow::bail;

pub mod common_regex;
pub mod percent_encoding;

use proxmox_schema::{
    api, const_regex, ApiStringFormat, ApiType, ArraySchema, Schema, StringSchema, ReturnType,
};
use proxmox_time::parse_daily_duration;

#[rustfmt::skip]
#[macro_export]
macro_rules! PROXMOX_SAFE_ID_REGEX_STR { () => { r"(?:[A-Za-z0-9_][A-Za-z0-9._\-]*)" }; }

#[rustfmt::skip]
#[macro_export]
macro_rules! BACKUP_ID_RE { () => (r"[A-Za-z0-9_][A-Za-z0-9._\-]*") }

#[rustfmt::skip]
#[macro_export]
macro_rules! BACKUP_TYPE_RE { () => (r"(?:host|vm|ct)") }

#[rustfmt::skip]
#[macro_export]
macro_rules! BACKUP_TIME_RE { () => (r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z") }

#[rustfmt::skip]
#[macro_export]
macro_rules! SNAPSHOT_PATH_REGEX_STR {
    () => (
        concat!(r"(", BACKUP_TYPE_RE!(), ")/(", BACKUP_ID_RE!(), ")/(", BACKUP_TIME_RE!(), r")")
    );
}

mod acl;
pub use acl::*;

mod datastore;
pub use datastore::*;

mod human_byte;
pub use human_byte::HumanByte;

mod jobs;
pub use jobs::*;

mod key_derivation;
pub use key_derivation::{Kdf, KeyInfo};

mod network;
pub use network::*;

#[macro_use]
mod userid;
pub use userid::Authid;
pub use userid::Userid;
pub use userid::{Realm, RealmRef};
pub use userid::{Tokenname, TokennameRef};
pub use userid::{Username, UsernameRef};
pub use userid::{PROXMOX_GROUP_ID_SCHEMA, PROXMOX_TOKEN_ID_SCHEMA, PROXMOX_TOKEN_NAME_SCHEMA};

#[macro_use]
mod user;
pub use user::*;

pub use proxmox_schema::upid::*;

mod crypto;
pub use crypto::{CryptMode, Fingerprint, bytes_as_fingerprint};

pub mod file_restore;

mod openid;
pub use openid::*;

mod remote;
pub use remote::*;

mod tape;
pub use tape::*;

mod traffic_control;
pub use traffic_control::*;

mod zfs;
pub use zfs::*;


#[rustfmt::skip]
#[macro_use]
mod local_macros {
    macro_rules! DNS_LABEL { () => (r"(?:[a-zA-Z0-9](?:[a-zA-Z0-9\-]*[a-zA-Z0-9])?)") }
    macro_rules! DNS_NAME { () => (concat!(r"(?:(?:", DNS_LABEL!() , r"\.)*", DNS_LABEL!(), ")")) }
    macro_rules! CIDR_V4_REGEX_STR { () => (concat!(r"(?:", IPV4RE!(), r"/\d{1,2})$")) }
    macro_rules! CIDR_V6_REGEX_STR { () => (concat!(r"(?:", IPV6RE!(), r"/\d{1,3})$")) }
    macro_rules! DNS_ALIAS_LABEL { () => (r"(?:[a-zA-Z0-9_](?:[a-zA-Z0-9\-]*[a-zA-Z0-9])?)") }
    macro_rules! DNS_ALIAS_NAME {
        () => (concat!(r"(?:(?:", DNS_ALIAS_LABEL!() , r"\.)*", DNS_ALIAS_LABEL!(), ")"))
    }
}

const_regex! {
    pub IP_V4_REGEX = concat!(r"^", IPV4RE!(), r"$");
    pub IP_V6_REGEX = concat!(r"^", IPV6RE!(), r"$");
    pub IP_REGEX = concat!(r"^", IPRE!(), r"$");
    pub CIDR_V4_REGEX =  concat!(r"^", CIDR_V4_REGEX_STR!(), r"$");
    pub CIDR_V6_REGEX =  concat!(r"^", CIDR_V6_REGEX_STR!(), r"$");
    pub CIDR_REGEX =  concat!(r"^(?:", CIDR_V4_REGEX_STR!(), "|",  CIDR_V6_REGEX_STR!(), r")$");
    pub HOSTNAME_REGEX = r"^(?:[a-zA-Z0-9](?:[a-zA-Z0-9\-]*[a-zA-Z0-9])?)$";
    pub DNS_NAME_REGEX =  concat!(r"^", DNS_NAME!(), r"$");
    pub DNS_ALIAS_REGEX =  concat!(r"^", DNS_ALIAS_NAME!(), r"$");
    pub DNS_NAME_OR_IP_REGEX = concat!(r"^(?:", DNS_NAME!(), "|",  IPRE!(), r")$");

    pub SHA256_HEX_REGEX = r"^[a-f0-9]{64}$"; // fixme: define in common_regex ?

    pub PASSWORD_REGEX = r"^[[:^cntrl:]]*$"; // everything but control characters

    pub UUID_REGEX = r"^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$";

    pub SYSTEMD_DATETIME_REGEX = r"^\d{4}-\d{2}-\d{2}( \d{2}:\d{2}(:\d{2})?)?$"; //  fixme: define in common_regex ?

    pub FINGERPRINT_SHA256_REGEX = r"^(?:[0-9a-fA-F][0-9a-fA-F])(?::[0-9a-fA-F][0-9a-fA-F]){31}$";

    /// Regex for safe identifiers.
    ///
    /// This
    /// [article](https://dwheeler.com/essays/fixing-unix-linux-filenames.html)
    /// contains further information why it is reasonable to restict
    /// names this way. This is not only useful for filenames, but for
    /// any identifier command line tools work with.
    pub PROXMOX_SAFE_ID_REGEX = concat!(r"^", PROXMOX_SAFE_ID_REGEX_STR!(), r"$");

    pub SINGLE_LINE_COMMENT_REGEX = r"^[[:^cntrl:]]*$";

    pub BACKUP_REPO_URL_REGEX = concat!(
        r"^^(?:(?:(",
        USER_ID_REGEX_STR!(), "|", APITOKEN_ID_REGEX_STR!(),
        ")@)?(",
        DNS_NAME!(), "|",  IPRE_BRACKET!(),
        "):)?(?:([0-9]{1,5}):)?(", PROXMOX_SAFE_ID_REGEX_STR!(), r")$"
    );

    pub BLOCKDEVICE_NAME_REGEX = r"^(:?(:?h|s|x?v)d[a-z]+)|(:?nvme\d+n\d+)$";
    pub SUBSCRIPTION_KEY_REGEX = concat!(r"^pbs(?:[cbsp])-[0-9a-f]{10}$");
}

pub const IP_V4_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&IP_V4_REGEX);
pub const IP_V6_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&IP_V6_REGEX);
pub const IP_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&IP_REGEX);
pub const CIDR_V4_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&CIDR_V4_REGEX);
pub const CIDR_V6_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&CIDR_V6_REGEX);
pub const CIDR_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&CIDR_REGEX);
pub const PVE_CONFIG_DIGEST_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&SHA256_HEX_REGEX);
pub const PASSWORD_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&PASSWORD_REGEX);
pub const UUID_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&UUID_REGEX);
pub const BLOCKDEVICE_NAME_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&BLOCKDEVICE_NAME_REGEX);
pub const SUBSCRIPTION_KEY_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&SUBSCRIPTION_KEY_REGEX);
pub const SYSTEMD_DATETIME_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&SYSTEMD_DATETIME_REGEX);
pub const HOSTNAME_FORMAT: ApiStringFormat = ApiStringFormat::Pattern(&HOSTNAME_REGEX);

pub const DNS_ALIAS_FORMAT: ApiStringFormat =
    ApiStringFormat::Pattern(&DNS_ALIAS_REGEX);

pub const DAILY_DURATION_FORMAT: ApiStringFormat =
    ApiStringFormat::VerifyFn(|s| parse_daily_duration(s).map(drop));

pub const SEARCH_DOMAIN_SCHEMA: Schema =
    StringSchema::new("Search domain for host-name lookup.").schema();

pub const FIRST_DNS_SERVER_SCHEMA: Schema =
    StringSchema::new("First name server IP address.")
    .format(&IP_FORMAT)
    .schema();

pub const SECOND_DNS_SERVER_SCHEMA: Schema =
    StringSchema::new("Second name server IP address.")
    .format(&IP_FORMAT)
    .schema();

pub const THIRD_DNS_SERVER_SCHEMA: Schema =
    StringSchema::new("Third name server IP address.")
    .format(&IP_FORMAT)
    .schema();

pub const HOSTNAME_SCHEMA: Schema = StringSchema::new("Hostname (as defined in RFC1123).")
    .format(&HOSTNAME_FORMAT)
    .schema();

pub const DNS_NAME_FORMAT: ApiStringFormat =
    ApiStringFormat::Pattern(&DNS_NAME_REGEX);

pub const DNS_NAME_OR_IP_FORMAT: ApiStringFormat =
    ApiStringFormat::Pattern(&DNS_NAME_OR_IP_REGEX);

pub const DNS_NAME_OR_IP_SCHEMA: Schema = StringSchema::new("DNS name or IP address.")
    .format(&DNS_NAME_OR_IP_FORMAT)
    .schema();

pub const NODE_SCHEMA: Schema = StringSchema::new("Node name (or 'localhost')")
    .format(&ApiStringFormat::VerifyFn(|node| {
        if node == "localhost" || node == proxmox_sys::nodename() {
            Ok(())
        } else {
            bail!("no such node '{}'", node);
        }
    }))
    .schema();

pub const TIME_ZONE_SCHEMA: Schema = StringSchema::new(
    "Time zone. The file '/usr/share/zoneinfo/zone.tab' contains the list of valid names.")
    .format(&SINGLE_LINE_COMMENT_FORMAT)
    .min_length(2)
    .max_length(64)
    .schema();

pub const BLOCKDEVICE_NAME_SCHEMA: Schema = StringSchema::new("Block device name (/sys/block/<name>).")
    .format(&BLOCKDEVICE_NAME_FORMAT)
    .min_length(3)
    .max_length(64)
    .schema();

pub const DISK_ARRAY_SCHEMA: Schema = ArraySchema::new(
    "Disk name list.", &BLOCKDEVICE_NAME_SCHEMA)
    .schema();

pub const DISK_LIST_SCHEMA: Schema = StringSchema::new(
    "A list of disk names, comma separated.")
    .format(&ApiStringFormat::PropertyString(&DISK_ARRAY_SCHEMA))
    .schema();

pub const PASSWORD_SCHEMA: Schema = StringSchema::new("Password.")
    .format(&PASSWORD_FORMAT)
    .min_length(1)
    .max_length(1024)
    .schema();

pub const PBS_PASSWORD_SCHEMA: Schema = StringSchema::new("User Password.")
    .format(&PASSWORD_FORMAT)
    .min_length(5)
    .max_length(64)
    .schema();

pub const REALM_ID_SCHEMA: Schema = StringSchema::new("Realm name.")
    .format(&PROXMOX_SAFE_ID_FORMAT)
    .min_length(2)
    .max_length(32)
    .schema();

pub const FINGERPRINT_SHA256_FORMAT: ApiStringFormat =
    ApiStringFormat::Pattern(&FINGERPRINT_SHA256_REGEX);

pub const CERT_FINGERPRINT_SHA256_SCHEMA: Schema =
    StringSchema::new("X509 certificate fingerprint (sha256).")
        .format(&FINGERPRINT_SHA256_FORMAT)
        .schema();

pub const PROXMOX_SAFE_ID_FORMAT: ApiStringFormat =
    ApiStringFormat::Pattern(&PROXMOX_SAFE_ID_REGEX);

pub const SINGLE_LINE_COMMENT_FORMAT: ApiStringFormat =
    ApiStringFormat::Pattern(&SINGLE_LINE_COMMENT_REGEX);

pub const SINGLE_LINE_COMMENT_SCHEMA: Schema = StringSchema::new("Comment (single line).")
    .format(&SINGLE_LINE_COMMENT_FORMAT)
    .schema();

pub const SUBSCRIPTION_KEY_SCHEMA: Schema = StringSchema::new("Proxmox Backup Server subscription key.")
    .format(&SUBSCRIPTION_KEY_FORMAT)
    .min_length(15)
    .max_length(16)
    .schema();

pub const SERVICE_ID_SCHEMA: Schema = StringSchema::new("Service ID.")
    .max_length(256)
    .schema();

pub const PROXMOX_CONFIG_DIGEST_SCHEMA: Schema = StringSchema::new(
    "Prevent changes if current configuration file has different \
    SHA256 digest. This can be used to prevent concurrent \
    modifications.",
)
.format(&PVE_CONFIG_DIGEST_FORMAT)
.schema();

/// API schema format definition for repository URLs
pub const BACKUP_REPO_URL: ApiStringFormat = ApiStringFormat::Pattern(&BACKUP_REPO_URL_REGEX);


// Complex type definitions


#[api()]
#[derive(Default, Serialize, Deserialize)]
/// Storage space usage information.
pub struct StorageStatus {
    /// Total space (bytes).
    pub total: u64,
    /// Used space (bytes).
    pub used: u64,
    /// Available space (bytes).
    pub avail: u64,
}

pub const PASSWORD_HINT_SCHEMA: Schema = StringSchema::new("Password hint.")
    .format(&SINGLE_LINE_COMMENT_FORMAT)
    .min_length(1)
    .max_length(64)
    .schema();


#[api]
#[derive(Deserialize, Serialize)]
/// RSA public key information
pub struct RsaPubKeyInfo {
    /// Path to key (if stored in a file)
    #[serde(skip_serializing_if="Option::is_none")]
    pub path: Option<String>,
    /// RSA exponent
    pub exponent: String,
    /// Hex-encoded RSA modulus
    pub modulus: String,
    /// Key (modulus) length in bits
    pub length: usize,
}

#[cfg(not(target_arch="wasm32"))]
impl std::convert::TryFrom<openssl::rsa::Rsa<openssl::pkey::Public>> for RsaPubKeyInfo {
    type Error = anyhow::Error;

    fn try_from(value: openssl::rsa::Rsa<openssl::pkey::Public>) -> Result<Self, Self::Error> {
        let modulus = value.n().to_hex_str()?.to_string();
        let exponent = value.e().to_dec_str()?.to_string();
        let length = value.size() as usize * 8;

        Ok(Self {
            path: None,
            exponent,
            modulus,
            length,
        })
    }
}

#[api()]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
/// Describes a package for which an update is available.
pub struct APTUpdateInfo {
    /// Package name
    pub package: String,
    /// Package title
    pub title: String,
    /// Package architecture
    pub arch: String,
    /// Human readable package description
    pub description: String,
    /// New version to be updated to
    pub version: String,
    /// Old version currently installed
    pub old_version: String,
    /// Package origin
    pub origin: String,
    /// Package priority in human-readable form
    pub priority: String,
    /// Package section
    pub section: String,
    /// URL under which the package's changelog can be retrieved
    pub change_log_url: String,
    /// Custom extra field for additional package information
    #[serde(skip_serializing_if="Option::is_none")]
    pub extra_info: Option<String>,
}


#[api()]
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// Node Power command type.
pub enum NodePowerCommand {
    /// Restart the server
    Reboot,
    /// Shutdown the server
    Shutdown,
}


#[api()]
#[derive(Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStateType {
    /// Ok
    OK,
    /// Warning
    Warning,
    /// Error
    Error,
    /// Unknown
    Unknown,
}

#[api(
    properties: {
        upid: { schema: UPID::API_SCHEMA },
    },
)]
#[derive(Serialize, Deserialize)]
/// Task properties.
pub struct TaskListItem {
    pub upid: String,
    /// The node name where the task is running on.
    pub node: String,
    /// The Unix PID
    pub pid: i64,
    /// The task start time (Epoch)
    pub pstart: u64,
    /// The task start time (Epoch)
    pub starttime: i64,
    /// Worker type (arbitrary ASCII string)
    pub worker_type: String,
    /// Worker ID (arbitrary ASCII string)
    pub worker_id: Option<String>,
    /// The authenticated entity who started the task
    pub user: String,
    /// The task end time (Epoch)
    #[serde(skip_serializing_if="Option::is_none")]
    pub endtime: Option<i64>,
    /// Task end status
    #[serde(skip_serializing_if="Option::is_none")]
    pub status: Option<String>,
}

pub const NODE_TASKS_LIST_TASKS_RETURN_TYPE: ReturnType = ReturnType {
    optional: false,
    schema: &ArraySchema::new(
        "A list of tasks.",
        &TaskListItem::API_SCHEMA,
    ).schema(),
};

#[api()]
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
/// RRD consolidation mode
pub enum RRDMode {
    /// Maximum
    Max,
    /// Average
    Average,
}

#[api()]
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// RRD time frame
pub enum RRDTimeFrame {
    /// Hour
    Hour,
    /// Day
    Day,
    /// Week
    Week,
    /// Month
    Month,
    /// Year
    Year,
    /// Decade (10 years)
    Decade,
}
