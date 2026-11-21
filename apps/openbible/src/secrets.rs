//! The Christian Open Library token (ADR 0024) in the operating system's credential store:
//! Keychain on macOS, Credential Manager on Windows, the Secret Service on Linux. The app
//! keeps it nowhere else. Where no credential store answers, the engine keeps the token for
//! the session only and says so.

use obs_engine::col_get::SecretStore;

const SERVICE: &str = "OpenBible";
const ACCOUNT: &str = "github-token";

/// The OS credential store, behind the engine's [`SecretStore`].
#[derive(Debug, Default)]
pub struct OsKeychain;

impl OsKeychain {
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())
    }
}

impl SecretStore for OsKeychain {
    fn get(&self) -> Result<Option<String>, String> {
        match Self::entry()?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, secret: &str) -> Result<(), String> {
        Self::entry()?
            .set_password(secret)
            .map_err(|e| e.to_string())
    }

    fn delete(&self) -> Result<(), String> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
