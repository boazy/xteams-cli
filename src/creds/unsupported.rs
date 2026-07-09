//! Fallback for platforms with no cookie implementation (neither macOS nor
//! Windows). Cookie extraction is unavailable here; `xteams auth login`
//! (device-code) is the only auth path and needs no cookies, so every entry
//! point fails cleanly with `UnsupportedPlatform` rather than failing to build.

use std::path::PathBuf;

use eyre::Result;

use crate::error::CredsError;

/// No key material on unsupported platforms.
pub type CookieKey = ();

pub fn default_cookies_path() -> Result<PathBuf> {
    Err(CredsError::UnsupportedPlatform.into())
}

pub fn derive_cookie_key() -> Result<CookieKey> {
    Err(CredsError::UnsupportedPlatform.into())
}

pub fn decrypt_value(_enc: &[u8], _key: &CookieKey) -> Option<String> {
    None
}
