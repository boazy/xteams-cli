//! Pure builders for the m365 MSAL token cache (`~/.cli-m365-msal.json`), letting m365
//! silently renew Graph tokens from an injected FOCI refresh token. Key derivation
//! mirrors @azure/msal-node (all keys lowercased):
//!   credential  = [homeAccountId, env, credentialType, familyId|clientId, realm, target, scheme]
//!   account     = [homeAccountId, env, realm]
//!   appmetadata = ["appmetadata", env, clientId]

use base64::Engine as _;
use serde_json::{Map, Value, json};

const ENV: &str = "login.microsoftonline.com";
const FAMILY: &str = "1";

pub fn account_key(oid: &str, tid: &str) -> String {
    format!("{oid}.{tid}-{ENV}-{tid}").to_lowercase()
}

pub fn refresh_token_key(oid: &str, tid: &str) -> String {
    format!("{oid}.{tid}-{ENV}-refreshtoken-{FAMILY}---").to_lowercase()
}

pub fn app_metadata_key(client_id: &str) -> String {
    format!("appmetadata-{ENV}-{client_id}").to_lowercase()
}

pub fn build_cache(oid: &str, tid: &str, upn: &str, refresh_token: &str, client_id: &str) -> Value {
    let hoid = format!("{oid}.{tid}").to_lowercase();
    let client_info =
        base64::engine::general_purpose::STANDARD.encode(json!({ "uid": oid, "utid": tid }).to_string());

    let mut account = Map::new();
    account.insert(
        account_key(oid, tid),
        json!({
            "home_account_id": hoid.clone(),
            "environment": ENV,
            "realm": tid,
            "local_account_id": oid,
            "username": upn,
            "authority_type": "MSSTS",
            "name": upn,
            "client_info": client_info,
        }),
    );

    let mut refresh = Map::new();
    refresh.insert(
        refresh_token_key(oid, tid),
        json!({
            "home_account_id": hoid.clone(),
            "environment": ENV,
            "credential_type": "RefreshToken",
            "client_id": client_id,
            "secret": refresh_token,
            "family_id": FAMILY,
        }),
    );

    let mut app = Map::new();
    app.insert(
        app_metadata_key(client_id),
        json!({ "client_id": client_id, "environment": ENV, "family_id": FAMILY }),
    );

    let mut cache = Map::new();
    cache.insert("Account".to_owned(), Value::Object(account));
    cache.insert("RefreshToken".to_owned(), Value::Object(refresh));
    cache.insert("AppMetadata".to_owned(), Value::Object(app));
    cache.insert("AccessToken".to_owned(), Value::Object(Map::new()));
    cache.insert("IdToken".to_owned(), Value::Object(Map::new()));
    Value::Object(cache)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const OID: &str = "71ef4b45-6c24-432d-b22b-d97e53a3da1a";
    const TID: &str = "af559f8b-54dc-49fe-8006-3cc5f2201ef3";
    const CID: &str = "1fec8e78-bce4-4aaf-ab1b-5451cc387264";

    #[test]
    fn keys_match_msal_derivation() {
        assert_eq!(account_key(OID, TID), format!("{OID}.{TID}-login.microsoftonline.com-{TID}"));
        assert_eq!(
            refresh_token_key(OID, TID),
            format!("{OID}.{TID}-login.microsoftonline.com-refreshtoken-1---")
        );
        assert_eq!(app_metadata_key(CID), format!("appmetadata-login.microsoftonline.com-{CID}"));
    }

    #[test]
    fn keys_are_lowercased() {
        assert_eq!(app_metadata_key("ABC-DEF"), "appmetadata-login.microsoftonline.com-abc-def");
    }

    #[test]
    fn build_cache_has_family_refresh_token_and_account() {
        let c = build_cache(OID, TID, "u@c.com", "the-rt", CID);
        let rt = &c["RefreshToken"][refresh_token_key(OID, TID)];
        assert_eq!(rt["secret"], json!("the-rt"));
        assert_eq!(rt["client_id"], json!(CID));
        assert_eq!(rt["family_id"], json!("1"));
        assert_eq!(rt["credential_type"], json!("RefreshToken"));

        let acct = &c["Account"][account_key(OID, TID)];
        assert_eq!(acct["local_account_id"], json!(OID));
        assert_eq!(acct["authority_type"], json!("MSSTS"));
        assert_eq!(acct["home_account_id"], json!(format!("{OID}.{TID}")));

        let am = &c["AppMetadata"][app_metadata_key(CID)];
        assert_eq!(am["family_id"], json!("1"));
        assert_eq!(am["client_id"], json!(CID));
    }
}
