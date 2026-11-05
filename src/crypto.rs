use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::TryRngCore;
use zeroize::{Zeroize, Zeroizing};

use crate::vault::Vault;

const MAGIC: &[u8; 4] = b"PK01";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
/// magic + mem + iterations + lanes + salt + nonce
const HEADER_LEN: usize = 4 + 4 + 4 + 4 + SALT_LEN + NONCE_LEN;
const MAX_FILE_LEN: usize = 8 * 1024 * 1024;

/// OWASP interactive Argon2id: 19 MiB, 2 iterations, 1 lane.
const DEFAULT_KDF: KdfParams = KdfParams {
    mem_kib: 19 * 1024,
    iterations: 2,
    lanes: 1,
};

#[derive(Clone, Copy)]
pub struct KdfParams {
    pub mem_kib: u32,
    pub iterations: u32,
    pub lanes: u32,
}

pub struct Session {
    key: Zeroizing<[u8; KEY_LEN]>,
    salt: [u8; SALT_LEN],
    kdf: KdfParams,
}

#[derive(Debug)]
pub enum CryptoError {
    TooShort,
    BadMagic,
    BadParams,
    WrongPassword,
    Json,
}

impl CryptoError {
    pub fn user_message(&self) -> String {
        match self {
            CryptoError::WrongPassword => "wrong password".to_owned(),
            CryptoError::Json => "couldn't read the vault contents".to_owned(),
            CryptoError::TooShort | CryptoError::BadMagic | CryptoError::BadParams => {
                "the vault file is damaged or is not a passkeep vault".to_owned()
            }
        }
    }
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .expect("the operating system random number generator is unavailable");
    bytes
}

pub fn create(vault: &Vault, password: &str) -> Result<(Vec<u8>, Session), CryptoError> {
    let salt: [u8; SALT_LEN] = random_bytes();
    let session = Session {
        key: derive_key(password, &salt, &DEFAULT_KDF)?,
        salt,
        kdf: DEFAULT_KDF,
    };
    let file = seal(vault, &session)?;
    Ok((file, session))
}

pub fn open(file: &[u8], password: &str) -> Result<(Vault, Session), CryptoError> {
    let parsed = parse(file)?;
    let key = derive_key(password, &parsed.salt, &parsed.kdf)?;
    let vault = decrypt(file, &parsed, &key)?;
    Ok((
        vault,
        Session {
            key,
            salt: parsed.salt,
            kdf: parsed.kdf,
        },
    ))
}

pub fn seal(vault: &Vault, session: &Session) -> Result<Vec<u8>, CryptoError> {
    let mut plaintext = Zeroizing::new(serde_json::to_vec(vault).map_err(|_| CryptoError::Json)?);
    let nonce_bytes: [u8; NONCE_LEN] = random_bytes();
    let cipher =
        Aes256Gcm::new_from_slice(session.key.as_slice()).map_err(|_| CryptoError::BadParams)?;
    let aad = aad_bytes(&session.kdf, &session.salt);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext.as_slice(),
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::BadParams)?;
    plaintext.zeroize();

    let mut file = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    file.extend_from_slice(MAGIC);
    file.extend_from_slice(&session.kdf.mem_kib.to_le_bytes());
    file.extend_from_slice(&session.kdf.iterations.to_le_bytes());
    file.extend_from_slice(&session.kdf.lanes.to_le_bytes());
    file.extend_from_slice(&session.salt);
    file.extend_from_slice(&nonce_bytes);
    file.extend_from_slice(&ciphertext);
    Ok(file)
}

struct Parsed {
    kdf: KdfParams,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
}

fn parse(file: &[u8]) -> Result<Parsed, CryptoError> {
    if file.len() > MAX_FILE_LEN || file.len() < HEADER_LEN + 16 {
        return Err(CryptoError::TooShort);
    }
    if &file[0..4] != MAGIC {
        return Err(CryptoError::BadMagic);
    }
    let kdf = KdfParams {
        mem_kib: u32::from_le_bytes(file[4..8].try_into().unwrap()),
        iterations: u32::from_le_bytes(file[8..12].try_into().unwrap()),
        lanes: u32::from_le_bytes(file[12..16].try_into().unwrap()),
    };
    if !sane_kdf(kdf) {
        return Err(CryptoError::BadParams);
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&file[16..16 + SALT_LEN]);
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&file[32..32 + NONCE_LEN]);
    Ok(Parsed { kdf, salt, nonce })
}

fn decrypt(file: &[u8], parsed: &Parsed, key: &[u8; KEY_LEN]) -> Result<Vault, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::BadParams)?;
    let aad = aad_bytes(&parsed.kdf, &parsed.salt);
    let mut plaintext = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&parsed.nonce),
                Payload {
                    msg: &file[HEADER_LEN..],
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::WrongPassword)?,
    );
    let vault = serde_json::from_slice(&plaintext).map_err(|_| CryptoError::Json)?;
    plaintext.zeroize();
    Ok(vault)
}

fn derive_key(
    password: &str,
    salt: &[u8; SALT_LEN],
    kdf: &KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, CryptoError> {
    let params = Params::new(kdf.mem_kib, kdf.iterations, kdf.lanes, Some(KEY_LEN))
        .map_err(|_| CryptoError::BadParams)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key[..])
        .map_err(|_| CryptoError::BadParams)?;
    Ok(key)
}

fn aad_bytes(kdf: &KdfParams, salt: &[u8; SALT_LEN]) -> [u8; 28] {
    let mut aad = [0u8; 28];
    aad[0..4].copy_from_slice(&kdf.mem_kib.to_le_bytes());
    aad[4..8].copy_from_slice(&kdf.iterations.to_le_bytes());
    aad[8..12].copy_from_slice(&kdf.lanes.to_le_bytes());
    aad[12..28].copy_from_slice(salt);
    aad
}

fn sane_kdf(kdf: KdfParams) -> bool {
    (8 * 1024..=256 * 1024).contains(&kdf.mem_kib)
        && (1..=16).contains(&kdf.iterations)
        && (1..=4).contains(&kdf.lanes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Entry;

    fn sample() -> Vault {
        Vault {
            entries: vec![Entry {
                id: "abc".to_owned(),
                label: "GitHub".to_owned(),
                url: String::new(),
                login: "octocat".to_owned(),
                password: "s3cret".to_owned(),
            }],
        }
    }

    #[test]
    fn roundtrip_and_wrong_password() {
        let vault = sample();
        let (file, session) = create(&vault, "correct horse battery").unwrap();
        let (opened, _) = open(&file, "correct horse battery").unwrap();
        assert_eq!(opened.entries[0].login, "octocat");
        assert_eq!(opened.entries[0].password, "s3cret");
        assert!(matches!(
            open(&file, "wrong password"),
            Err(CryptoError::WrongPassword)
        ));

        let mut changed = opened;
        changed.entries[0].password = "next".to_owned();
        let saved = seal(&changed, &session).unwrap();
        assert_ne!(saved, file);
        let (again, _) = open(&saved, "correct horse battery").unwrap();
        assert_eq!(again.entries[0].password, "next");
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let (mut file, _) = create(&sample(), "correct horse battery").unwrap();
        let last = file.len() - 1;
        file[last] ^= 0xff;
        assert!(matches!(
            open(&file, "correct horse battery"),
            Err(CryptoError::WrongPassword)
        ));
    }

    #[test]
    fn tampered_kdf_params_are_rejected() {
        let (mut file, _) = create(&sample(), "correct horse battery").unwrap();
        file[4] ^= 0xff;
        assert!(open(&file, "correct horse battery").is_err());
    }
}
