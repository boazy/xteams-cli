//! macOS credential extraction for New Teams (Teams 2.0, Edge WebView2).
//!
//! The cookie key is derived (PBKDF2-HMAC-SHA1) from the "Microsoft Teams Safe
//! Storage" secret in the login Keychain, then used for AES-128-CBC `v10`/`v11`
//! decryption. This mirrors the proven PoC in `poc/`.

use std::path::PathBuf;

use aes::Aes128;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use eyre::Result;
use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
use security_framework::passwords::get_generic_password;

use crate::error::CredsError;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// AES-128 key derived from the Keychain secret.
pub type CookieKey = [u8; 16];

const KEYCHAIN_SERVICE: &str = "Microsoft Teams Safe Storage";
const KEYCHAIN_ACCOUNT: &str = "Microsoft Teams";
const PBKDF2_SALT: &[u8] = b"saltysalt";
const PBKDF2_ROUNDS: u32 = 1003;
/// `errSecItemNotFound`, defined locally to avoid pulling in `security-framework-sys`
/// just for one stable OSStatus constant.
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

pub fn default_cookies_path() -> Result<PathBuf> {
    let home = etcetera::home_dir()?;
    Ok(home.join(
        "Library/Containers/com.microsoft.teams2/Data/Library/Application Support/\
Microsoft/MSTeams/EBWebView/WV2Profile_tfw/Cookies",
    ))
}

pub fn derive_cookie_key() -> Result<CookieKey> {
    let secret = keychain_secret()?;
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(&secret, PBKDF2_SALT, PBKDF2_ROUNDS, &mut key);
    Ok(key)
}

/// Decrypt one Chromium `v10`/`v11` cookie value; `None` if not decryptable/printable.
pub fn decrypt_value(enc: &[u8], key: &CookieKey) -> Option<String> {
    let body = enc.strip_prefix(b"v10").or_else(|| enc.strip_prefix(b"v11"))?;
    let iv = [b' '; 16];
    let plain = Aes128CbcDec::new_from_slices(key, &iv)
        .ok()?
        .decrypt_padded_vec_mut::<Pkcs7>(body)
        .ok()?;
    super::plaintext_to_cookie(&plain)
}

/// Read the "Safe Storage" secret from the login Keychain in-process: by service
/// + account, then a service-only fallback.
fn keychain_secret() -> Result<Vec<u8>> {
    match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
        Ok(secret) => Ok(secret),
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => keychain_secret_by_service()
            .ok_or_else(|| CredsError::Keychain("item not found".to_owned()).into()),
        Err(e) => Err(CredsError::Keychain(e.to_string()).into()),
    }
}

fn keychain_secret_by_service() -> Option<Vec<u8>> {
    let results = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(KEYCHAIN_SERVICE)
        .load_data(true)
        .search()
        .ok()?;
    results.into_iter().find_map(|r| match r {
        SearchResult::Data(data) => Some(data),
        _ => None,
    })
}
