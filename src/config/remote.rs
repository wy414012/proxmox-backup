use failure::*;
use lazy_static::lazy_static;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

use proxmox::api::{api, schema::*};

use proxmox::tools::{fs::replace_file, fs::CreateOptions};

use crate::api2::types::*;
use crate::section_config::{SectionConfig, SectionConfigData, SectionConfigPlugin};

lazy_static! {
    static ref CONFIG: SectionConfig = init();
}

pub const REMOTE_PASSWORD_SCHEMA: Schema = StringSchema::new("Password or auth token for remote host.")
    .format(&PASSWORD_FORMAT)
    .min_length(1)
    .max_length(1024)
    .schema();

#[api(
    properties: {
        comment: {
            optional: true,
            schema: SINGLE_LINE_COMMENT_SCHEMA,
        },
        host: {
            schema: DNS_NAME_OR_IP_SCHEMA,
        },
        userid: {
            schema: PROXMOX_USER_ID_SCHEMA,
        },
        password: {
            schema: REMOTE_PASSWORD_SCHEMA,
        },
        fingerprint: {
            optional: true,
            schema: CERT_FINGERPRINT_SHA256_SCHEMA,
        },
    }
)]
#[derive(Serialize,Deserialize)]
/// Remote properties.
pub struct Remote {
    #[serde(skip_serializing_if="Option::is_none")]
    pub comment: Option<String>,
    pub host: String,
    pub userid: String,
    pub password: String,
    #[serde(skip_serializing_if="Option::is_none")]
    pub fingerprint: Option<String>,
}

fn init() -> SectionConfig {
    let obj_schema = match Remote::API_SCHEMA {
        Schema::Object(ref obj_schema) => obj_schema,
        _ => unreachable!(),
    };

    let plugin = SectionConfigPlugin::new("remote".to_string(), obj_schema);
    let mut config = SectionConfig::new(&REMOTE_ID_SCHEMA);
    config.register_plugin(plugin);

    config
}

pub const REMOTE_CFG_FILENAME: &str = "/etc/proxmox-backup/remote.cfg";
pub const REMOTE_CFG_LOCKFILE: &str = "/etc/proxmox-backup/.remote.lck";

pub fn config() -> Result<(SectionConfigData, [u8;32]), Error> {
    let content = match std::fs::read_to_string(REMOTE_CFG_FILENAME) {
        Ok(c) => c,
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                String::from("")
            } else {
                bail!("unable to read '{}' - {}", REMOTE_CFG_FILENAME, err);
            }
        }
    };

    let digest = openssl::sha::sha256(content.as_bytes());
    let data = CONFIG.parse(REMOTE_CFG_FILENAME, &content)?;
    Ok((data, digest))
}

pub fn save_config(config: &SectionConfigData) -> Result<(), Error> {
    let raw = CONFIG.write(REMOTE_CFG_FILENAME, &config)?;

    let backup_user = crate::backup::backup_user()?;
    let mode = nix::sys::stat::Mode::from_bits_truncate(0o0640);
    // set the correct owner/group/permissions while saving file
    // owner(rw) = root, group(r)= backup
    let options = CreateOptions::new()
        .perm(mode)
        .owner(nix::unistd::ROOT)
        .group(backup_user.gid);

    replace_file(REMOTE_CFG_FILENAME, raw.as_bytes(), options)?;

    Ok(())
}

// shell completion helper
pub fn complete_remote_name(_arg: &str, _param: &HashMap<String, String>) -> Vec<String> {
    match config() {
        Ok((data, _digest)) => data.sections.iter().map(|(id, _)| id.to_string()).collect(),
        Err(_) => return vec![],
    }
}
