//! Windows DPAPI-backed secret storage.
//!
//! Provider credentials (cookies, OAuth tokens, an IPTV account password) must
//! never be written to `settings.json` in the clear. `CryptProtectData` ties the
//! ciphertext to the current Windows user profile, so a copied file is useless
//! on another machine or account.
//!
//! Deliberately no `windows` crate: two `extern "system"` declarations against
//! `crypt32.dll` do the job and cost nothing in binary size. That is worth more
//! than the ergonomics of a generated binding here.

use std::ffi::c_void;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum SecretError {
    Unsupported,
    Platform { code: i32 },
    Io(String),
    Empty,
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretError::Unsupported => write!(f, "secret storage is only implemented on Windows"),
            SecretError::Platform { code } => write!(f, "DPAPI failed with error {code}"),
            SecretError::Io(message) => write!(f, "{message}"),
            SecretError::Empty => write!(f, "secret is empty"),
        }
    }
}

impl std::error::Error for SecretError {}

/// Per-user vault for provider secrets, one file per key.
#[derive(Debug, Clone)]
pub struct SecretVault {
    dir: PathBuf,
}

impl SecretVault {
    pub fn open(root: &Path) -> Result<Self, SecretError> {
        let dir = root.join("secrets");
        fs::create_dir_all(&dir).map_err(|err| SecretError::Io(err.to_string()))?;
        Ok(Self { dir })
    }

    fn path_for(&self, key: &str) -> PathBuf {
        let safe: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.bin"))
    }

    pub fn store(&self, key: &str, plaintext: &[u8]) -> Result<(), SecretError> {
        if plaintext.is_empty() {
            return Err(SecretError::Empty);
        }
        let blob = protect(plaintext)?;
        fs::write(self.path_for(key), blob).map_err(|err| SecretError::Io(err.to_string()))
    }

    pub fn load(&self, key: &str) -> Result<Vec<u8>, SecretError> {
        let bytes = fs::read(self.path_for(key)).map_err(|err| SecretError::Io(err.to_string()))?;
        unprotect(&bytes)
    }

    pub fn exists(&self, key: &str) -> bool {
        self.path_for(key).exists()
    }

    pub fn remove(&self, key: &str) -> Result<(), SecretError> {
        let path = self.path_for(key);
        if path.exists() {
            fs::remove_file(path).map_err(|err| SecretError::Io(err.to_string()))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DPAPI
// ---------------------------------------------------------------------------

#[repr(C)]
struct DataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

#[cfg(windows)]
mod platform {
    use super::{c_void, DataBlob};

    pub const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    #[link(name = "crypt32")]
    extern "system" {
        pub fn CryptProtectData(
            data_in: *const DataBlob,
            description: *const u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;

        pub fn CryptUnprotectData(
            data_in: *const DataBlob,
            description: *mut *mut u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn LocalFree(mem: *mut c_void) -> *mut c_void;
    }
}

#[cfg(windows)]
pub fn protect(plaintext: &[u8]) -> Result<Vec<u8>, SecretError> {
    unsafe {
        let mut input = DataBlob {
            cb_data: plaintext.len() as u32,
            pb_data: plaintext.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = platform::CryptProtectData(
            &mut input as *mut DataBlob,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            platform::CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 {
            return Err(SecretError::Platform {
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        let bytes = std::slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec();
        platform::LocalFree(output.pb_data as *mut c_void);
        Ok(bytes)
    }
}

#[cfg(windows)]
pub fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>, SecretError> {
    unsafe {
        let mut input = DataBlob {
            cb_data: ciphertext.len() as u32,
            pb_data: ciphertext.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = platform::CryptUnprotectData(
            &mut input as *mut DataBlob,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            platform::CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 {
            return Err(SecretError::Platform {
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        let bytes = std::slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec();
        platform::LocalFree(output.pb_data as *mut c_void);
        Ok(bytes)
    }
}

#[cfg(not(windows))]
pub fn protect(_plaintext: &[u8]) -> Result<Vec<u8>, SecretError> {
    Err(SecretError::Unsupported)
}

#[cfg(not(windows))]
pub fn unprotect(_ciphertext: &[u8]) -> Result<Vec<u8>, SecretError> {
    Err(SecretError::Unsupported)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_secret_through_dpapi() {
        let secret = b"refresh-token-abc123";
        let blob = protect(secret).expect("protect");
        assert_ne!(&blob[..], &secret[..], "ciphertext must not be plaintext");
        assert_eq!(unprotect(&blob).expect("unprotect"), secret);
    }

    #[test]
    fn vault_persists_and_removes() {
        let mut root = std::env::temp_dir();
        root.push(format!("cs-vault-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let vault = SecretVault::open(&root).expect("vault");
        vault.store("youtube", b"cookie=1").expect("store");
        assert!(vault.exists("youtube"));
        assert_eq!(vault.load("youtube").expect("load"), b"cookie=1");
        vault.remove("youtube").expect("remove");
        assert!(!vault.exists("youtube"));
        let _ = fs::remove_dir_all(&root);
    }
}
