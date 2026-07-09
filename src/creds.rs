//! Credential extraction for New Teams (Teams 2.0, Edge WebView2).
//!
//! New Teams keeps its web session in a Chromium "EBWebView" profile whose auth
//! cookies live in a SQLite DB with encrypted `v10`/`v11` values. Reading that DB
//! is platform-independent; deriving the key and the cipher are not, so each OS
//! has its own `imp` submodule:
//!
//! - `macos` — Keychain secret → PBKDF2-HMAC-SHA1 → AES-128-CBC (tested).
//! - `windows` — `Local State` key → DPAPI unwrap → AES-256-GCM (**untested**).
//! - anything else — unsupported: [`load_cookies`] fails with a clear error that
//!   points at `xteams auth login` (the FRT path needs no cookies).

use std::path::{Path, PathBuf};

use eyre::{Context, Result};

use crate::error::CredsError;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

#[cfg(not(any(target_os = "macos", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "macos", windows)))]
use unsupported as imp;

/// The two auth cookies the internal Teams APIs consume.
#[derive(Debug, Clone)]
pub struct TeamsCookies {
    /// `authtoken` (teams.microsoft.com): wraps the AAD bearer token.
    pub authtoken: String,
    /// `skypetoken_asm`: the Skype/chat-service token.
    pub skypetoken: String,
}

/// Default cookie store for the signed-in work profile, resolved per platform.
pub fn default_cookies_path() -> Result<PathBuf> {
    imp::default_cookies_path()
}

/// Load and decrypt the Teams auth cookies from the given `Cookies` SQLite DB.
///
/// Key derivation + per-value decryption are platform-specific (`imp`); the
/// SQLite read and value post-processing are shared across platforms.
pub fn load_cookies(cookies_db: &Path) -> Result<TeamsCookies> {
    let key = imp::derive_cookie_key()?;
    let rows = read_encrypted_cookies(cookies_db)?;

    let mut authtoken = None;
    let mut skypetoken = None;
    for (name, enc) in rows {
        match name.as_str() {
            "authtoken" if authtoken.is_none() => authtoken = imp::decrypt_value(&enc, &key),
            "skypetoken_asm" if skypetoken.is_none() => skypetoken = imp::decrypt_value(&enc, &key),
            _ => {}
        }
    }

    Ok(TeamsCookies {
        authtoken: authtoken.ok_or(CredsError::CookieMissing("authtoken"))?,
        skypetoken: skypetoken.ok_or(CredsError::CookieMissing("skypetoken_asm"))?,
    })
}

/// Turn a decrypted cookie plaintext into a printable string, tolerating the
/// optional 32-byte `SHA256(host)` prefix Chromium >= M127 prepends. Shared by
/// each platform `imp` (child modules can see this parent-private helper).
#[cfg(any(target_os = "macos", windows))]
fn plaintext_to_cookie(plain: &[u8]) -> Option<String> {
    const DOMAIN_HASH_LEN: usize = 32;
    let stripped = plain.get(DOMAIN_HASH_LEN..).unwrap_or(&[]);
    for candidate in [plain, stripped] {
        if let Ok(text) = std::str::from_utf8(candidate)
            && !text.is_empty()
            && text.chars().all(|c| !c.is_control())
        {
            return Some(text.to_owned());
        }
    }
    None
}

/// Copy the (possibly locked) cookie DB to a temp file and read the two rows.
fn read_encrypted_cookies(cookies_db: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    if !cookies_db.exists() {
        return Err(CredsError::CookieDbMissing(cookies_db.display().to_string()).into());
    }
    let tmp = std::env::temp_dir().join(format!("xteams-cookies-{}.db", std::process::id()));
    std::fs::copy(cookies_db, &tmp)
        .with_context(|| format!("copying cookie DB from {}", cookies_db.display()))?;

    let result = query_cookies(&tmp);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn query_cookies(db: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let conn = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("opening cookie DB copy")?;
    let mut stmt = conn.prepare(
        "SELECT name, encrypted_value FROM cookies \
         WHERE name IN ('authtoken', 'skypetoken_asm')",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
