use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::crypto;

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub url: String,
    pub login: String,
    pub password: String,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.label.zeroize();
        self.url.zeroize();
        self.login.zeroize();
        self.password.zeroize();
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Vault {
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl Vault {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

pub fn new_id() -> String {
    crypto::random_bytes::<16>()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Vault file. Override with `PASSKEEP_VAULT` when developing.
pub fn vault_path() -> PathBuf {
    if let Some(path) = std::env::var_os("PASSKEEP_VAULT") {
        return PathBuf::from(path);
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(dir).join("passkeep").join("vault.pk");
    }
    PathBuf::from("vault.pk")
}

/// Writes ciphertext via a temp file and keeps the previous file as `vault.bak`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let tmp = path.with_extension("tmp");
    let backup = path.with_extension("bak");
    fs::write(&tmp, bytes)?;

    if path.exists() {
        let _ = fs::remove_file(&backup);
        fs::rename(path, &backup)?;
    }

    if let Err(error) = fs::rename(&tmp, path) {
        if backup.exists() && !path.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(error);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_previous_bytes() {
        let dir = std::env::temp_dir().join(format!("passkeep-atomic-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vault.pk");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read(path.with_extension("bak")).unwrap(), b"one");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn entry_without_a_website_still_loads() {
        let entry: Entry =
            serde_json::from_str(r#"{"id":"a","label":"GitHub","login":"me","password":"secret"}"#)
                .unwrap();
        assert!(entry.url.is_empty());
    }
}
