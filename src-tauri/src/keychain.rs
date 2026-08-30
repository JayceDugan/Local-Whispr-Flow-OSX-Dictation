//! OS Keychain access for the ASR API token.
//!
//! The token never leaves Rust: commands expose `save_token` / `delete_token`
//! / `has_token` only — there is no getter that crosses the IPC boundary, so
//! the React layer can never hold or leak it.

use security_framework::passwords;

const SERVICE: &str = "ai-lab-asr-osx";
const ACCOUNT: &str = "asr-api-token";

pub fn save_token(token: &str) -> Result<(), crate::error::ClientError> {
    passwords::set_generic_password(SERVICE, ACCOUNT, token.as_bytes())
        .map_err(|e| crate::error::ClientError::audio(format!("keychain save failed: {e}")))
}

/// Read the token for request signing (Rust-internal use only).
/// `Ok(None)` means no token configured — requests go out unauthenticated.
pub fn load_token() -> Result<Option<String>, crate::error::ClientError> {
    match passwords::get_generic_password(SERVICE, ACCOUNT) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        // errSecItemNotFound — simply no token stored yet.
        Err(e) if e.code() == security_framework_sys::base::errSecItemNotFound => Ok(None),
        Err(e) => Err(crate::error::ClientError::audio(format!(
            "keychain read failed: {e}"
        ))),
    }
}

pub fn delete_token() -> Result<(), crate::error::ClientError> {
    match passwords::delete_generic_password(SERVICE, ACCOUNT) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == security_framework_sys::base::errSecItemNotFound => Ok(()),
        Err(e) => Err(crate::error::ClientError::audio(format!(
            "keychain delete failed: {e}"
        ))),
    }
}

pub fn has_token() -> bool {
    matches!(load_token(), Ok(Some(t)) if !t.is_empty())
}
