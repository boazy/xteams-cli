//! macOS credential extraction for New Teams (Teams 2.0, Edge WebView2).
//!
//! New Teams stores its web credentials in a Chromium "EBWebView" profile:
//! cookies live in a SQLite DB with AES-128-CBC `v10` encrypted values, and the
//! key is derived (PBKDF2-HMAC-SHA1) from the "Microsoft Teams Safe Storage"
//! secret in the login Keychain. This mirrors the proven PoC in `poc/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use aes::Aes128;
use anyhow::{Context, Result, anyhow};
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};

use crate::error::CredsError;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

const KEYCHAIN_SERVICE: &str = "Microsoft Teams Safe Storage";
const KEYCHAIN_ACCOUNT: &str = "Microsoft Teams";
const PBKDF2_SALT: &[u8] = b"saltysalt";
const PBKDF2_ROUNDS: u32 = 1003;
const DOMAIN_HASH_LEN: usize = 32;

/// The two auth cookies the internal Teams APIs consume.
#[derive(Debug, Clone)]
pub struct TeamsCookies {
    /// `authtoken` (teams.microsoft.com): wraps the AAD bearer token.
    pub authtoken: String,
    /// `skypetoken_asm`: the Skype/chat-service token.
    pub skypetoken: String,
}

/// Default cookie store for the signed-in work profile (`WV2Profile_tfw`).
pub fn default_cookies_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot resolve home directory"))?;
    Ok(home.join(
        "Library/Containers/com.microsoft.teams2/Data/Library/Application Support/\
Microsoft/MSTeams/EBWebView/WV2Profile_tfw/Cookies",
    ))
}

/// Load and decrypt the Teams auth cookies from the given `Cookies` SQLite DB.
pub fn load_cookies(cookies_db: &Path) -> Result<TeamsCookies> {
    let key = derive_cookie_key()?;
    let rows = read_encrypted_cookies(cookies_db)?;

    let mut authtoken = None;
    let mut skypetoken = None;
    for (name, enc) in rows {
        match name.as_str() {
            "authtoken" if authtoken.is_none() => authtoken = decrypt_value(&enc, &key),
            "skypetoken_asm" if skypetoken.is_none() => skypetoken = decrypt_value(&enc, &key),
            _ => {}
        }
    }

    Ok(TeamsCookies {
        authtoken: authtoken.ok_or(CredsError::CookieMissing("authtoken"))?,
        skypetoken: skypetoken.ok_or(CredsError::CookieMissing("skypetoken_asm"))?,
    })
}

/// Read the Safe Storage secret from the Keychain and derive the AES key.
fn derive_cookie_key() -> Result<[u8; 16]> {
    let secret = keychain_secret()?;
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(secret.trim().as_bytes(), PBKDF2_SALT, PBKDF2_ROUNDS, &mut key);
    Ok(key)
}

fn keychain_secret() -> Result<String> {
    let primary =
        run_security(&["find-generic-password", "-w", "-s", KEYCHAIN_SERVICE, "-a", KEYCHAIN_ACCOUNT])?;
    if let Some(secret) = primary {
        return Ok(secret);
    }
    // Retry without the account filter in case the acct attribute differs.
    let fallback = run_security(&["find-generic-password", "-w", "-s", KEYCHAIN_SERVICE])?;
    fallback.ok_or_else(|| CredsError::Keychain("item not found".to_owned()).into())
}

/// Run `/usr/bin/security`; `Ok(None)` means "not found", `Err` means it failed.
fn run_security(args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("/usr/bin/security")
        .args(args)
        .output()
        .context("failed to run /usr/bin/security")?;
    if output.status.success() {
        let secret = String::from_utf8(output.stdout).context("keychain secret is not UTF-8")?;
        Ok(Some(secret))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("could not be found") {
            Ok(None)
        } else {
            Err(CredsError::Keychain(stderr.trim().to_owned()).into())
        }
    }
}

/// Decrypt one Chromium `v10` cookie value; `None` if not decryptable/printable.
fn decrypt_value(enc: &[u8], key: &[u8; 16]) -> Option<String> {
    let body = enc.strip_prefix(b"v10").or_else(|| enc.strip_prefix(b"v11"))?;
    let iv = [b' '; 16];
    let plain = Aes128CbcDec::new_from_slices(key, &iv)
        .ok()?
        .decrypt_padded_vec_mut::<Pkcs7>(body)
        .ok()?;
    // Chromium >= M127 prepends a 32-byte SHA256(host) before the value.
    let stripped = plain.get(DOMAIN_HASH_LEN..).unwrap_or(&[]);
    for candidate in [plain.as_slice(), stripped] {
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
