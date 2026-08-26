use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rcgen::{CertifiedKey, PublicKeyData, generate_simple_self_signed};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

use crate::i18n::tr;

#[derive(Clone)]
pub struct PcIdentity {
    pub pc_id: String,
    pub pc_name: String,
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub spki_sha256: String,
}

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    pc_id: String,
    pc_name: String,
    cert_der: String,
    #[serde(default)]
    key_der: Option<String>,
    #[serde(default)]
    protected_key_der: Option<String>,
    spki_sha256: String,
}

pub fn data_dir() -> io::Result<PathBuf> {
    let root = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is unavailable"))?;
    let path = root.join("FlowType");
    fs::create_dir_all(&path)?;
    Ok(path)
}

impl PcIdentity {
    pub fn load_or_create() -> Result<Self, Box<dyn std::error::Error>> {
        let path = data_dir()?.join("identity-v1.json");
        if path.exists() {
            let mut stored: StoredIdentity = serde_json::from_slice(&fs::read(&path)?)?;
            let key_der = if let Some(protected) = stored.protected_key_der.as_deref() {
                unprotect(&STANDARD.decode(protected)?)?
            } else {
                let plaintext = STANDARD.decode(
                    stored
                        .key_der
                        .as_deref()
                        .ok_or("identity private key is missing")?,
                )?;
                stored.protected_key_der = Some(STANDARD.encode(protect(&plaintext)?));
                stored.key_der = None;
                fs::write(&path, serde_json::to_vec_pretty(&stored)?)?;
                plaintext
            };
            return Ok(Self {
                pc_id: stored.pc_id,
                pc_name: stored.pc_name,
                cert_der: STANDARD.decode(stored.cert_der)?,
                key_der,
                spki_sha256: stored.spki_sha256,
            });
        }

        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["flowtype.local".to_owned()])?;
        let spki_sha256 = STANDARD.encode(Sha256::digest(signing_key.subject_public_key_info()));
        let identity = Self {
            pc_id: Uuid::new_v4().to_string(),
            pc_name: env::var("COMPUTERNAME")
                .unwrap_or_else(|_| tr("Windows 电脑", "Windows PC").to_owned()),
            cert_der: cert.der().to_vec(),
            key_der: signing_key.serialize_der(),
            spki_sha256,
        };
        let stored = StoredIdentity {
            pc_id: identity.pc_id.clone(),
            pc_name: identity.pc_name.clone(),
            cert_der: STANDARD.encode(&identity.cert_der),
            key_der: None,
            protected_key_der: Some(STANDARD.encode(protect(&identity.key_der)?)),
            spki_sha256: identity.spki_sha256.clone(),
        };
        fs::write(path, serde_json::to_vec_pretty(&stored)?)?;
        Ok(identity)
    }

    pub fn save_pc_name(name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err("computer name must contain 1 to 64 characters".into());
        }
        let path = data_dir()?.join("identity-v1.json");
        let mut stored: StoredIdentity = serde_json::from_slice(&fs::read(&path)?)?;
        stored.pc_name = name.to_owned();
        fs::write(path, serde_json::to_vec_pretty(&stored)?)?;
        Ok(())
    }
}

fn protect(plaintext: &[u8]) -> io::Result<Vec<u8>> {
    crypt_data(plaintext, true)
}

fn unprotect(ciphertext: &[u8]) -> io::Result<Vec<u8>> {
    crypt_data(ciphertext, false)
}

fn crypt_data(input: &[u8], protecting: bool) -> io::Result<Vec<u8>> {
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let success = unsafe {
        if protecting {
            CryptProtectData(
                &input_blob,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input_blob,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe { LocalFree(output.pbData.cast()) };
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{protect, unprotect};

    #[test]
    fn dpapi_round_trip_is_bound_to_the_current_user() {
        let plaintext = b"private-key-test";
        let protected = protect(plaintext).unwrap();
        assert_ne!(protected, plaintext);
        assert_eq!(unprotect(&protected).unwrap(), plaintext);
    }
}
