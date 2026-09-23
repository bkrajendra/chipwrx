//! OS keychain storage for the optional `ANTHROPIC_API_KEY` (`NFR-S1`, `DATA-MODEL.md` §13:
//! "never a config file"). Backed by the `keyring` crate — Windows Credential Manager,
//! macOS Keychain, and the Linux Secret Service. Never logged, never returned to the
//! frontend (`commands::secrets` only reports whether a key is set, never its value).

use crate::error::AppError;

const SERVICE: &str = "com.vibehardware.app";
const ACCOUNT: &str = "ANTHROPIC_API_KEY";

fn entry() -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(store_err)
}

fn store_err(e: keyring::Error) -> AppError {
    AppError::SecretStoreFailed { message: e.to_string() }
}

pub fn set_anthropic_api_key(key: &str) -> Result<(), AppError> {
    entry()?.set_password(key).map_err(store_err)
}

/// `None` if no key has ever been set — not an error (`keyring::Error::NoEntry`).
pub fn get_anthropic_api_key() -> Result<Option<String>, AppError> {
    match entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(store_err(e)),
    }
}

pub fn has_anthropic_api_key() -> Result<bool, AppError> {
    Ok(get_anthropic_api_key()?.is_some())
}

/// A no-op (not an error) if nothing was set — mirrors `get`'s treatment of `NoEntry`.
pub fn clear_anthropic_api_key() -> Result<(), AppError> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(store_err(e)),
    }
}
