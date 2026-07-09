//! Token exchange: turn the desktop app's cookies into a usable API session.
//!
//! The `authtoken` cookie wraps an AAD bearer (audience `api.spaces.skype.com`).
//! Posting it to the Teams `authz` endpoint returns a fresh skype token plus the
//! `regionGtms` service map (chat service host, etc.) — no region hardcoding.

use std::collections::BTreeMap;
use std::path::Path;

use eyre::Result;
use base64::Engine as _;
use serde::Deserialize;

use crate::creds::{self, TeamsCookies};
use crate::error::AuthError;

mod authenticator;
mod device_code;
mod oauth;
mod store;

pub use authenticator::{Authenticator, logout};

const AUTHZ_URL: &str = "https://authsvc.teams.microsoft.com/v1.0/authz";
const USER_AGENT: &str = "xteams-cli/0.1 (Teams-compatible)";

/// A ready-to-use API session derived from the local credentials.
#[derive(Debug, Clone)]
pub struct Session {
    pub skype_token: String,
    pub aad_bearer: String,
    pub region: String,
    pub chat_service: String,
    pub gtms: BTreeMap<String, String>,
    pub identity: Identity,
}

/// Identity decoded from the AAD bearer's JWT claims.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub upn: Option<String>,
    pub name: Option<String>,
    pub tenant: Option<String>,
}

/// Identity claims pulled from a Graph access-token JWT, used to seed other CLIs'
/// credential stores (they key on `oid`/`tid`, which `Identity` does not carry).
#[derive(Debug, Clone, Default)]
pub struct GraphIdentity {
    pub oid: Option<String>,
    pub upn: Option<String>,
    pub tid: Option<String>,
}

/// Build a reqwest client, load local cookies, and establish a session.
pub async fn connect(cookies: Option<&Path>) -> Result<(reqwest::Client, Session)> {
    let path = match cookies {
        Some(p) => p.to_path_buf(),
        None => creds::default_cookies_path()?,
    };
    let cookies = creds::load_cookies(&path)?;
    let client = build_client()?;
    let session = Session::establish(&client, &cookies).await?;
    Ok((client, session))
}

pub fn build_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}

const DEFAULT_TENANT: &str = "organizations";

pub async fn load_authenticator() -> Result<Authenticator> {
    Authenticator::load(build_client()?, DEFAULT_TENANT).await
}

pub async fn login_authenticator() -> Result<Authenticator> {
    Authenticator::login(build_client()?, DEFAULT_TENANT).await
}

impl Session {
    pub async fn establish(client: &reqwest::Client, cookies: &TeamsCookies) -> Result<Self> {
        let aad_bearer = extract_bearer(&cookies.authtoken).ok_or(AuthError::BearerMissing)?;
        let resp = client
            .post(AUTHZ_URL)
            .bearer_auth(&aad_bearer)
            .json(&serde_json::json!({}))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::Authz { status: status.as_u16(), body: truncate(&body, 240) }.into());
        }

        let parsed: AuthzResponse = resp.json().await?;
        let skype_token = parsed
            .skype_token()
            .or_else(|| (!cookies.skypetoken.is_empty()).then(|| cookies.skypetoken.clone()))
            .ok_or(AuthError::NoSkypeToken)?;
        let gtms = parsed.gtms();
        let chat_service = gtms.get("chatService").cloned().ok_or(AuthError::NoChatService)?;
        let identity = identity_from_jwt(&aad_bearer);

        Ok(Self {
            skype_token,
            aad_bearer,
            region: parsed.region.unwrap_or_default(),
            chat_service,
            gtms,
            identity,
        })
    }
}

/// Pull the JWT out of the `Bearer=<jwt>&Origin=...` cookie value.
fn extract_bearer(authtoken: &str) -> Option<String> {
    let decoded = urlencoding::decode(authtoken).ok()?;
    for part in decoded.split('&') {
        if let Some(jwt) = part.strip_prefix("Bearer=") {
            return Some(jwt.to_owned());
        }
    }
    if decoded.matches('.').count() == 2 && decoded.starts_with("ey") {
        return Some(decoded.into_owned());
    }
    None
}

fn identity_from_jwt(jwt: &str) -> Identity {
    let Some(claims) = decode_claims(jwt) else {
        return Identity::default();
    };
    let get = |key: &str| claims.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    Identity {
        upn: get("upn").or_else(|| get("preferred_username")),
        name: get("name"),
        tenant: get("tid"),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Decode a JWT's audience and remaining lifetime in seconds; `(None, None)` when the
/// token is not a decodable JWT (e.g. an opaque refresh token).
pub fn jwt_audience_and_ttl(jwt: &str) -> (Option<String>, Option<i64>) {
    let Some(claims) = decode_claims(jwt) else {
        return (None, None);
    };
    let audience = claims.get("aud").and_then(|v| v.as_str()).map(str::to_owned);
    let ttl = claims.get("exp").and_then(serde_json::Value::as_i64).map(|exp| exp - now_secs());
    (audience, ttl)
}

fn decode_claims(jwt: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Extract the seed-relevant identity claims (`oid`, `upn`, `tid`) from a Graph
/// access-token JWT. Returns defaults for a non-decodable token.
pub fn graph_identity(jwt: &str) -> GraphIdentity {
    let Some(claims) = decode_claims(jwt) else {
        return GraphIdentity::default();
    };
    let get = |key: &str| claims.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    GraphIdentity {
        oid: get("oid"),
        upn: get("upn").or_else(|| get("preferred_username")),
        tid: get("tid"),
    }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[derive(Debug, Deserialize)]
struct AuthzResponse {
    region: Option<String>,
    tokens: Option<Tokens>,
    #[serde(rename = "skypeToken")]
    skype_token: Option<String>,
    #[serde(rename = "regionGtms")]
    region_gtms: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct Tokens {
    #[serde(rename = "skypeToken")]
    skype_token: Option<String>,
}

impl AuthzResponse {
    fn skype_token(&self) -> Option<String> {
        self.tokens
            .as_ref()
            .and_then(|t| t.skype_token.clone())
            .or_else(|| self.skype_token.clone())
    }

    fn gtms(&self) -> BTreeMap<String, String> {
        self.region_gtms
            .as_ref()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_jwt(aud: &str, exp: i64) -> String {
        let payload = serde_json::json!({ "aud": aud, "exp": exp }).to_string();
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        format!("eyJhbGciOiJub25lIn0.{encoded}.sig")
    }

    #[test]
    fn jwt_audience_and_ttl_reads_aud_and_remaining_minutes() {
        let (aud, ttl) = jwt_audience_and_ttl(&fake_jwt("https://graph.microsoft.com", now_secs() + 3600));
        assert_eq!(aud.as_deref(), Some("https://graph.microsoft.com"));
        let ttl = ttl.expect("ttl should decode");
        assert!(ttl > 3500 && ttl <= 3600, "expected ~1h ttl, got {ttl}");
    }

    #[test]
    fn jwt_audience_and_ttl_on_opaque_refresh_token_is_none() {
        let (aud, ttl) = jwt_audience_and_ttl("1.AcoOpaqueRefreshTokenNotAJwt.q0qw");
        assert_eq!(aud, None);
        assert_eq!(ttl, None);
    }

    fn fake_jwt_claims(claims: serde_json::Value) -> String {
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
        format!("eyJhbGciOiJub25lIn0.{encoded}.sig")
    }

    #[test]
    fn graph_identity_extracts_oid_upn_tid() {
        let jwt = fake_jwt_claims(serde_json::json!({
            "oid": "11111111-1111-1111-1111-111111111111",
            "upn": "user@contoso.com",
            "tid": "22222222-2222-2222-2222-222222222222",
        }));
        let id = graph_identity(&jwt);
        assert_eq!(id.oid.as_deref(), Some("11111111-1111-1111-1111-111111111111"));
        assert_eq!(id.upn.as_deref(), Some("user@contoso.com"));
        assert_eq!(id.tid.as_deref(), Some("22222222-2222-2222-2222-222222222222"));
    }

    #[test]
    fn graph_identity_falls_back_to_preferred_username() {
        let jwt = fake_jwt_claims(serde_json::json!({
            "oid": "abc",
            "preferred_username": "pref@contoso.com",
            "tid": "def",
        }));
        let id = graph_identity(&jwt);
        assert_eq!(id.upn.as_deref(), Some("pref@contoso.com"));
        assert_eq!(id.oid.as_deref(), Some("abc"));
    }
}
