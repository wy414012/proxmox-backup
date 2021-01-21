use anyhow::{bail, Error};
use serde_json::Value;

use proxmox::{
    api::{
        api,
        ApiMethod,
        Router,
        RpcEnvironment,
    },
    tools::fs::open_file_locked,
};

use crate::{
    config::{
        tape_encryption_keys::{
            TAPE_KEYS_LOCKFILE,
            generate_tape_encryption_key,
            load_keys,
            load_key_configs,
            save_keys,
            save_key_configs,
            insert_key,
        },
    },
    api2::types::{
        TAPE_ENCRYPTION_KEY_FINGERPRINT_SCHEMA,
        PROXMOX_CONFIG_DIGEST_SCHEMA,
        PASSWORD_HINT_SCHEMA,
        KeyInfo,
        Kdf,
    },
    backup::{
        KeyConfig,
        Fingerprint,
    },
};

#[api(
    input: {
        properties: {},
    },
    returns: {
        description: "The list of tape encryption keys (with config digest).",
        type: Array,
        items: { type: KeyInfo },
    },
)]
/// List existing keys
pub fn list_keys(
    _param: Value,
    _info: &ApiMethod,
    mut rpcenv: &mut dyn RpcEnvironment,
) -> Result<Vec<KeyInfo>, Error> {

    let (key_map, digest) = load_key_configs()?;

    let mut list = Vec::new();

    for (_fingerprint, item) in key_map.iter() {
        list.push(item.into());
    }

    rpcenv["digest"] = proxmox::tools::digest_to_hex(&digest).into();

    Ok(list)
}

#[api(
    protected: true,
    input: {
        properties: {
            kdf: {
                type: Kdf,
                optional: true,
            },
            fingerprint: {
                schema: TAPE_ENCRYPTION_KEY_FINGERPRINT_SCHEMA,
            },
            password: {
                description: "The current password.",
                min_length: 5,
            },
            "new-password": {
                description: "The new password.",
                min_length: 5,
            },
            hint: {
                schema: PASSWORD_HINT_SCHEMA,
            },
            digest: {
                optional: true,
                schema: PROXMOX_CONFIG_DIGEST_SCHEMA,
            },
        },
    },
)]
/// Change the encryption key's password (and password hint).
pub fn change_passphrase(
    kdf: Option<Kdf>,
    password: String,
    new_password: String,
    hint: String,
    fingerprint: Fingerprint,
    digest: Option<String>,
    _rpcenv: &mut dyn RpcEnvironment
) -> Result<(), Error> {

    let kdf = kdf.unwrap_or_default();

    if let Kdf::None = kdf {
        bail!("Please specify a key derivation  funktion (none is not allowed here).");
    }

    let _lock = open_file_locked(
        TAPE_KEYS_LOCKFILE,
        std::time::Duration::new(10, 0),
        true,
    )?;

    let (mut config_map, expected_digest) = load_key_configs()?;

    if let Some(ref digest) = digest {
        let digest = proxmox::tools::hex_to_digest(digest)?;
        crate::tools::detect_modified_configuration_file(&digest, &expected_digest)?;
    }

    let key_config = match config_map.get(&fingerprint) {
        Some(key_config) => key_config,
        None => bail!("tape encryption key '{}' does not exist.", fingerprint),
    };

    let (key, created, fingerprint) = key_config.decrypt(&|| Ok(password.as_bytes().to_vec()))?;
    let mut new_key_config = KeyConfig::with_key(&key, new_password.as_bytes(), kdf)?;
    new_key_config.created = created; // keep original value
    new_key_config.fingerprint = Some(fingerprint.clone());
    new_key_config.hint = Some(hint);

    config_map.insert(fingerprint, new_key_config);

    save_key_configs(config_map)?;

    Ok(())
}

#[api(
    protected: true,
    input: {
        properties: {
            kdf: {
                type: Kdf,
                optional: true,
            },
            password: {
                description: "A secret password.",
                min_length: 5,
            },
            hint: {
                schema: PASSWORD_HINT_SCHEMA,
            },
        },
    },
    returns: {
        schema: TAPE_ENCRYPTION_KEY_FINGERPRINT_SCHEMA,
    },
)]
/// Create a new encryption key
pub fn create_key(
    kdf: Option<Kdf>,
    password: String,
    hint: String,
    _rpcenv: &mut dyn RpcEnvironment
) -> Result<Fingerprint, Error> {

    let kdf = kdf.unwrap_or_default();

    if let Kdf::None = kdf {
        bail!("Please specify a key derivation  funktion (none is not allowed here).");
    }

    let (key, mut key_config) = generate_tape_encryption_key(password.as_bytes(), kdf)?;
    key_config.hint = Some(hint);

    let fingerprint = key_config.fingerprint.clone().unwrap();

    insert_key(key, key_config, false)?;

    Ok(fingerprint)
}


#[api(
    input: {
        properties: {
            fingerprint: {
                schema: TAPE_ENCRYPTION_KEY_FINGERPRINT_SCHEMA,
            },
        },
    },
    returns: {
        type: KeyInfo,
    },
)]
/// Get key config (public key part)
pub fn read_key(
    fingerprint: Fingerprint,
    _rpcenv: &mut dyn RpcEnvironment,
) -> Result<KeyInfo, Error> {

    let (config_map, _digest) = load_key_configs()?;

    let key_config = match config_map.get(&fingerprint) {
        Some(key_config) => key_config,
        None => bail!("tape encryption key '{}' does not exist.", fingerprint),
    };

    if key_config.kdf.is_none() {
        bail!("found unencrypted key - internal error");
    }

    Ok(key_config.into())
}

#[api(
    protected: true,
    input: {
        properties: {
            fingerprint: {
                schema: TAPE_ENCRYPTION_KEY_FINGERPRINT_SCHEMA,
            },
            digest: {
                optional: true,
                schema: PROXMOX_CONFIG_DIGEST_SCHEMA,
            },
        },
    },
)]
/// Remove a encryption key from the database
///
/// Please note that you can no longer access tapes using this key.
pub fn delete_key(
    fingerprint: Fingerprint,
    digest: Option<String>,
    _rpcenv: &mut dyn RpcEnvironment,
) -> Result<(), Error> {

    let _lock = open_file_locked(
        TAPE_KEYS_LOCKFILE,
        std::time::Duration::new(10, 0),
        true,
    )?;

    let (mut config_map, expected_digest) = load_key_configs()?;
    let (mut key_map, _) = load_keys()?;

    if let Some(ref digest) = digest {
        let digest = proxmox::tools::hex_to_digest(digest)?;
        crate::tools::detect_modified_configuration_file(&digest, &expected_digest)?;
    }

    match config_map.get(&fingerprint) {
        Some(_) => { config_map.remove(&fingerprint); },
        None => bail!("tape encryption key '{}' does not exist.", fingerprint),
    }
    save_key_configs(config_map)?;

    key_map.remove(&fingerprint);
    save_keys(key_map)?;

    Ok(())
}

const ITEM_ROUTER: Router = Router::new()
    .get(&API_METHOD_READ_KEY)
    .put(&API_METHOD_CHANGE_PASSPHRASE)
    .delete(&API_METHOD_DELETE_KEY);

pub const ROUTER: Router = Router::new()
    .get(&API_METHOD_LIST_KEYS)
    .post(&API_METHOD_CREATE_KEY)
    .match_all("fingerprint", &ITEM_ROUTER);