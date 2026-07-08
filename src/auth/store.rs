//! macOS Keychain store for the FOCI refresh token (device-code path), via the same
//! `security-framework` crate as creds.rs. The token lives in our own item (service
//! `xteams`, account `foci-refresh`) — never a plaintext file.

use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

use crate::error::TokenStoreError;

const SERVICE: &str = "xteams";
const ACCOUNT: &str = "foci-refresh";
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

pub fn save_refresh_token(token: &str) -> Result<(), TokenStoreError> {
    save(ACCOUNT, token)
}

pub fn load_refresh_token() -> Result<Option<String>, TokenStoreError> {
    load(ACCOUNT)
}

pub fn delete_refresh_token() -> Result<(), TokenStoreError> {
    delete(ACCOUNT)
}

fn save(account: &str, token: &str) -> Result<(), TokenStoreError> {
    set_generic_password(SERVICE, account, token.as_bytes())
        .map_err(|e| TokenStoreError::Write(e.to_string()))
}

fn load(account: &str) -> Result<Option<String>, TokenStoreError> {
    match get_generic_password(SERVICE, account) {
        Ok(bytes) => Ok(Some(String::from_utf8(bytes).map_err(|e| TokenStoreError::Read(e.to_string()))?)),
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(e) => Err(TokenStoreError::Read(e.to_string())),
    }
}

fn delete(account: &str) -> Result<(), TokenStoreError> {
    match delete_generic_password(SERVICE, account) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
        Err(e) => Err(TokenStoreError::Delete(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_save_load_delete() {
        let account = format!("foci-refresh-test-{}", std::process::id());
        save(&account, "sample-refresh-token").expect("save should succeed");
        assert_eq!(load(&account).expect("load should succeed").as_deref(), Some("sample-refresh-token"));
        delete(&account).expect("delete should succeed");
        assert_eq!(load(&account).expect("load after delete should succeed"), None);
    }

    #[test]
    fn delete_missing_item_is_ok() {
        let account = format!("foci-refresh-absent-{}", std::process::id());
        assert!(delete(&account).is_ok());
        assert_eq!(load(&account).expect("load absent should succeed"), None);
    }
}
