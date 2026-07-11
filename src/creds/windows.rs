//! Windows credential extraction for New Teams (Teams 2.0, Edge WebView2).
//!
//! **UNTESTED** — implemented from public references but never run on Windows
//! (no Windows host was available). The flow mirrors macOS but with Chromium's
//! Windows crypto: the AES-256 key lives in the profile's `Local State`
//! (`os_crypt.encrypted_key`, base64 + DPAPI-wrapped), and cookie values are
//! AES-256-GCM (`v10` = 12-byte nonce ++ ciphertext ++ 16-byte tag). Caveats:
//!
//! - **App-Bound Encryption (`v20`):** Chromium >= 127 may instead store an
//!   `app_bound_encrypted_key`, wrapped with an extra SYSTEM/app-bound layer this
//!   code does NOT unwrap. If only that key is present, extraction fails with a
//!   clear error — sign in with `xteams auth login` (needs no cookies) instead.
//! - **File lock:** `ms-teams.exe` (its `msedgewebview2.exe` child) keeps the
//!   `Cookies` DB open, so the copy-to-temp read may fail unless Teams is closed.

use std::path::PathBuf;
use std::ptr;
use std::slice;

use aes_gcm::aead::{Aead, Nonce};
use aes_gcm::{Aes256Gcm, KeyInit};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use etcetera::base_strategy::{BaseStrategy, choose_base_strategy};
use eyre::Result;
use serde::Deserialize;
use windows_sys::Win32::Foundation::{HLOCAL, LocalFree};
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
};

use crate::error::CredsError;

/// AES-256 key unwrapped from the profile's `Local State`.
pub type CookieKey = [u8; 32];

const TEAMS_PACKAGE: &str = "MSTeams_8wekyb3d8bbwe";
const DPAPI_PREFIX: &[u8] = b"DPAPI";
const GCM_NONCE_LEN: usize = 12;
const AES256_KEY_LEN: usize = 32;

#[derive(Deserialize)]
struct LocalState {
    os_crypt: OsCrypt,
}

#[derive(Deserialize)]
struct OsCrypt {
    encrypted_key: Option<String>,
}

/// `%LOCALAPPDATA%\Packages\MSTeams_8wekyb3d8bbwe\LocalCache\Microsoft\MSTeams\EBWebView`.
fn ebwebview_dir() -> Result<PathBuf> {
    let local = choose_base_strategy()
        .map_err(|_| CredsError::Windows("cannot resolve %LOCALAPPDATA%".to_owned()))?
        .cache_dir();
    Ok(local
        .join("Packages")
        .join(TEAMS_PACKAGE)
        .join("LocalCache/Microsoft/MSTeams/EBWebView"))
}

/// Default cookie store for the signed-in work profile (`WV2Profile_tfw`).
///
/// Modern WebView2 (Chrome/Edge >= v96) keeps cookies under a `Network/`
/// subfolder; older installs keep them at the profile root. Prefer whichever
/// exists, defaulting to the modern path so a missing-DB error names it.
pub fn default_cookies_path() -> Result<PathBuf> {
    let profile = ebwebview_dir()?.join("WV2Profile_tfw");
    let modern = profile.join("Network").join("Cookies");
    let legacy = profile.join("Cookies");
    Ok(if legacy.exists() && !modern.exists() {
        legacy
    } else {
        modern
    })
}

pub fn derive_cookie_key() -> Result<CookieKey> {
    let path = ebwebview_dir()?.join("Local State");
    let bytes = std::fs::read(&path)
        .map_err(|e| CredsError::Windows(format!("reading {}: {e}", path.display())))?;
    let state: LocalState = serde_json::from_slice(&bytes)
        .map_err(|e| CredsError::Windows(format!("parsing Local State JSON: {e}")))?;
    let encoded = state.os_crypt.encrypted_key.ok_or_else(|| {
        CredsError::Windows(
            "Local State has no DPAPI 'encrypted_key' (app-bound v20 only?); not supported"
                .to_owned(),
        )
    })?;
    let wrapped = BASE64
        .decode(encoded.as_bytes())
        .map_err(|e| CredsError::Windows(format!("base64-decoding os_crypt key: {e}")))?;
    let blob = wrapped
        .strip_prefix(DPAPI_PREFIX)
        .ok_or_else(|| CredsError::Windows("os_crypt key missing 'DPAPI' prefix".to_owned()))?;
    let raw = dpapi_unprotect(blob)?;
    raw.as_slice().try_into().map_err(|_| {
        CredsError::Windows(format!("DPAPI key was {} bytes, expected {AES256_KEY_LEN}", raw.len()))
            .into()
    })
}

/// Decrypt one Chromium `v10`/`v11` (AES-256-GCM) cookie value; `None` if not
/// decryptable/printable.
pub fn decrypt_value(enc: &[u8], key: &CookieKey) -> Option<String> {
    let body = enc.strip_prefix(b"v10").or_else(|| enc.strip_prefix(b"v11"))?;
    let (nonce, ciphertext) = body.split_at_checked(GCM_NONCE_LEN)?;
    let nonce = Nonce::<Aes256Gcm>::try_from(nonce).ok()?;
    let cipher = Aes256Gcm::new_from_slice(key.as_slice()).ok()?;
    let plain = cipher.decrypt(&nonce, ciphertext).ok()?;
    super::plaintext_to_cookie(&plain)
}

/// Unwrap a user-scoped DPAPI blob (the profile key) via `CryptUnprotectData`.
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>> {
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    // SAFETY: `input` borrows `blob` only for this call and DPAPI does not retain
    // it. On success DPAPI allocates `output.pbData` (LocalAlloc); we copy those
    // `cbData` bytes out and release the buffer exactly once via `LocalFree`.
    // Every unused optional/out parameter is a null pointer, as the API permits.
    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(CredsError::Windows("CryptUnprotectData failed".to_owned()).into());
    }
    // SAFETY: on success DPAPI guarantees `pbData` addresses `cbData` init bytes.
    let secret = unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    // SAFETY: `pbData` came from DPAPI's LocalAlloc; freed once, after copying.
    unsafe { LocalFree(output.pbData as HLOCAL) };
    Ok(secret)
}
