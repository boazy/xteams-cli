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

mod oauth;
mod store;

const AUTHZ_URL: &str = "https://authsvc.teams.microsoft.com/v1.0/authz";
const USER_AGENT: &str = "xteams-cli/0.1 (Teams-compatible)";

/// A ready-to-use API session derived from the local credentials.
#[derive(Debug, Clone)]
pub struct Session {
    pub skype_token: String,
    pub region: String,
    pub chat_service: String,
    pub gtms: BTreeMap<String, String>,
    pub identity: Identity,
}

/// Identity + expiry decoded from the AAD bearer's JWT claims.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub upn: Option<String>,
    pub name: Option<String>,
    pub tenant: Option<String>,
    pub audience: Option<String>,
    pub expires_in_secs: Option<i64>,
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
    Ok(reqwest::Client::builder().user_agent(USER_AGENT).build()?)
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
    let exp = claims.get("exp").and_then(serde_json::Value::as_i64);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    Identity {
        upn: get("upn").or_else(|| get("preferred_username")),
        name: get("name"),
        tenant: get("tid"),
        audience: get("aud"),
        expires_in_secs: exp.map(|e| e - now),
    }
}

fn decode_claims(jwt: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
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
