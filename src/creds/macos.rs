//! macOS credential extraction for New Teams (Teams 2.0, Edge WebView2).
//!
//! The cookie key is derived (PBKDF2-HMAC-SHA1) from the "Microsoft Teams Safe
//! Storage" secret in the login Keychain, then used for AES-128-CBC `v10`/`v11`
//! decryption. This mirrors the proven PoC in `poc/`.

use std::path::PathBuf;

use aes::Aes128;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
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
    let body = enc
        .strip_prefix(b"v10")
        .or_else(|| enc.strip_prefix(b"v11"))?;
    let iv = [b' '; 16];
    let plain = Aes128CbcDec::new_from_slices(key, &iv)
        .ok()?
        .decrypt_padded_vec::<Pkcs7>(body)
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use cbc::cipher::{BlockModeEncrypt, block_padding::Pkcs7};

    type Aes128CbcEnc = cbc::Encryptor<Aes128>;

    /// Encrypt like Chromium does: AES-128-CBC, IV = 16 spaces, PKCS7, `v10` tag.
    fn encrypt_v10(key: &CookieKey, plain: &[u8]) -> Vec<u8> {
        let iv = [b' '; 16];
        let mut enc = b"v10".to_vec();
        enc.extend(
            Aes128CbcEnc::new_from_slices(key, &iv)
                .unwrap()
                .encrypt_padded_vec::<Pkcs7>(plain),
        );
        enc
    }

    #[test]
    fn decrypt_value_round_trips_v10_and_v11() {
        let key: CookieKey = [7u8; 16];
        let enc = encrypt_v10(&key, b"authtoken-value-123");
        assert_eq!(
            decrypt_value(&enc, &key).as_deref(),
            Some("authtoken-value-123")
        );
        // Same ciphertext under the `v11` tag decrypts identically.
        let mut v11 = enc.clone();
        v11[..3].copy_from_slice(b"v11");
        assert_eq!(
            decrypt_value(&v11, &key).as_deref(),
            Some("authtoken-value-123")
        );
    }

    #[test]
    fn decrypt_value_strips_m127_domain_hash() {
        let key: CookieKey = [9u8; 16];
        // 32-byte SHA256(host) prefix (with a control byte, as real hashes have) + cookie.
        let mut plain = vec![0x01u8; 32];
        plain.extend_from_slice(b"skypetoken-abc");
        let enc = encrypt_v10(&key, &plain);
        assert_eq!(decrypt_value(&enc, &key).as_deref(), Some("skypetoken-abc"));
    }

    #[test]
    fn decrypt_value_rejects_unknown_prefix() {
        let key: CookieKey = [3u8; 16];
        assert_eq!(decrypt_value(b"v20\x00\x01raw", &key), None);
    }
}
