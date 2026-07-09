//! Pure builders for the m365 CLI connection store (`~/.cli-m365-connection.json`).
//! No I/O: primitives in, serde-serializable values out for the store layer to write.

use std::collections::BTreeMap;

use serde::Serialize;

const GRAPH: &str = "https://graph.microsoft.com";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub active: bool,
    pub auth_type: &'static str,
    pub cloud_type: &'static str,
    pub certificate_type: u8,
    pub app_id: String,
    pub tenant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_name: Option<String>,
    pub identity_id: String,
    pub identity_tenant_id: String,
    pub access_tokens: BTreeMap<String, AccessToken>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessToken {
    pub expires_on: String,
    pub access_token: String,
}

pub fn build_connection(
    token: &str,
    expires_on: &str,
    upn: Option<&str>,
    oid: &str,
    tid: &str,
    app_id: &str,
    tenant: &str,
) -> Connection {
    let mut access_tokens = BTreeMap::new();
    access_tokens.insert(
        GRAPH.to_owned(),
        AccessToken { expires_on: expires_on.to_owned(), access_token: token.to_owned() },
    );
    Connection {
        active: true,
        auth_type: "deviceCode",
        cloud_type: "Public",
        certificate_type: 0,
        app_id: app_id.to_owned(),
        tenant: tenant.to_owned(),
        name: Some(oid.to_owned()),
        identity_name: upn.map(str::to_owned),
        identity_id: oid.to_owned(),
        identity_tenant_id: tid.to_owned(),
        access_tokens,
    }
}

pub fn all_connections_upsert(
    existing: Option<serde_json::Value>,
    conn_value: serde_json::Value,
    name: &str,
) -> serde_json::Value {
    let mut arr = match existing {
        Some(serde_json::Value::Array(items)) => items,
        _ => Vec::new(),
    };
    arr.retain(|c| c.get("name").and_then(serde_json::Value::as_str) != Some(name));
    arr.push(conn_value);
    serde_json::Value::Array(arr)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn builds_active_graph_connection() {
        let conn =
            build_connection("tok", "2026-07-09T15:30:00Z", Some("u@c.com"), "oid1", "tid1", "app1", "common");
        let v = serde_json::to_value(&conn).unwrap();
        assert_eq!(v["active"], serde_json::json!(true));
        assert_eq!(v["authType"], serde_json::json!("deviceCode"));
        assert_eq!(v["cloudType"], serde_json::json!("Public"));
        assert_eq!(v["certificateType"], serde_json::json!(0));
        assert_eq!(v["appId"], serde_json::json!("app1"));
        assert_eq!(v["tenant"], serde_json::json!("common"));
        assert_eq!(v["identityId"], serde_json::json!("oid1"));
        assert_eq!(v["identityTenantId"], serde_json::json!("tid1"));
        assert_eq!(v["identityName"], serde_json::json!("u@c.com"));
        assert_eq!(
            v["accessTokens"]["https://graph.microsoft.com"]["accessToken"],
            serde_json::json!("tok")
        );
        assert_eq!(
            v["accessTokens"]["https://graph.microsoft.com"]["expiresOn"],
            serde_json::json!("2026-07-09T15:30:00Z")
        );
    }

    #[test]
    fn upsert_creates_then_replaces_by_name() {
        let conn = build_connection("tok", "exp", None, "oidX", "tidX", "app", "common");
        let val = serde_json::to_value(&conn).unwrap();
        let created = all_connections_upsert(None, val.clone(), "oidX");
        assert_eq!(created.as_array().map(|a| a.len()), Some(1));
        let replaced = all_connections_upsert(Some(created), val, "oidX");
        assert_eq!(replaced.as_array().map(|a| a.len()), Some(1));
    }
}
