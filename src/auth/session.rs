//! The API `Session` (chat-service credentials + identity) and how it is established:
//! from an FRT-derived skype session, or as a fallback from desktop cookies. Also the
//! shared `authz` POST + response parsing used by both paths.

use std::collections::BTreeMap;

use eyre::Result;
use serde::Deserialize;
use serde_json::json;

use super::credential::{CachedCredential, SessionCredential};
use super::jwt::{Identity, identity_from_jwt};
use super::token_cache::StoredSkypeSession;
use crate::creds::TeamsCookies;
use crate::error::AuthError;

pub(crate) const AUTHZ_URL: &str = "https://authsvc.teams.microsoft.com/v1.0/authz";

/// A ready-to-use chat-service session, tagged with the credential that backs it so a
/// chat 401 can invalidate the right cache entry.
#[derive(Debug, Clone)]
pub struct Session {
    pub skype_token: String,
    pub region: String,
    pub chat_service: String,
    pub gtms: BTreeMap<String, String>,
    pub identity: Identity,
    pub credential: SessionCredential,
}

impl Session {
    /// Cookie fallback: extract the AAD bearer from `authtoken`, exchange it at authz.
    pub async fn establish(client: &reqwest::Client, cookies: &TeamsCookies) -> Result<Self> {
        let aad = extract_bearer(&cookies.authtoken).ok_or(AuthError::BearerMissing)?;
        let parsed = post_authz(client, &aad, None).await?;
        let skype_token = parsed
            .skype_token()
            .or_else(|| (!cookies.skypetoken.is_empty()).then(|| cookies.skypetoken.clone()))
            .ok_or(AuthError::NoSkypeToken)?;
        let gtms = parsed.gtms();
        let chat_service = gtms.get("chatService").cloned().ok_or(AuthError::NoChatService)?;
        Ok(Self {
            skype_token,
            region: parsed.region.unwrap_or_default(),
            chat_service,
            gtms,
            identity: identity_from_jwt(&aad),
            credential: SessionCredential::Cookie,
        })
    }

    /// FRT path: wrap an already-derived (cached or fresh) skype session + identity.
    pub fn from_skype_session(skype: &StoredSkypeSession, identity: Identity) -> Self {
        Self {
            skype_token: skype.skype_token.clone(),
            region: skype.region.clone(),
            chat_service: skype.chat_service.clone(),
            gtms: skype.gtms.clone(),
            identity,
            credential: SessionCredential::CachedSkype,
        }
    }
}

/// POST `bearer` to authz. `on_unauthorized` tags a 401 with the cached credential to
/// invalidate (FRT path); `None` (cookie path) maps 401 like any other authz failure.
pub(crate) async fn post_authz(
    client: &reqwest::Client,
    bearer: &str,
    on_unauthorized: Option<CachedCredential>,
) -> Result<AuthzResponse> {
    let resp = client.post(AUTHZ_URL).bearer_auth(bearer).json(&json!({})).send().await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(resp.json::<AuthzResponse>().await?);
    }
    let body = resp.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::UNAUTHORIZED
        && let Some(credential) = on_unauthorized
    {
        return Err(AuthError::AuthzUnauthorized { credential, body: truncate(&body, 240) }.into());
    }
    Err(AuthError::Authz { status: status.as_u16(), body: truncate(&body, 240) }.into())
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

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[derive(Debug, Deserialize)]
pub(crate) struct AuthzResponse {
    pub region: Option<String>,
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
    pub(crate) fn skype_token(&self) -> Option<String> {
        self.tokens.as_ref().and_then(|t| t.skype_token.clone()).or_else(|| self.skype_token.clone())
    }

    pub(crate) fn gtms(&self) -> BTreeMap<String, String> {
        self.region_gtms
            .as_ref()
            .map(|m| {
                m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned()))).collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn extract_bearer_from_cookie_and_bare_jwt() {
        let cookie = "Bearer%3DeyJhbGc.body.sig%26Origin%3Dhttps%3A%2F%2Fx";
        assert_eq!(extract_bearer(cookie).as_deref(), Some("eyJhbGc.body.sig"));
        assert_eq!(extract_bearer("eyaaa.bbb.ccc").as_deref(), Some("eyaaa.bbb.ccc"));
        assert_eq!(extract_bearer("not-a-token"), None);
    }

    #[test]
    fn authz_response_reads_skype_and_gtms() {
        let json = serde_json::json!({
            "region": "amer",
            "tokens": { "skypeToken": "sk-nested" },
            "regionGtms": { "chatService": "https://amer.ng.msg", "ignored": 5 },
        });
        let parsed: AuthzResponse = serde_json::from_value(json).expect("parse");
        assert_eq!(parsed.skype_token().as_deref(), Some("sk-nested"));
        assert_eq!(parsed.region.as_deref(), Some("amer"));
        let gtms = parsed.gtms();
        assert_eq!(gtms.get("chatService").map(String::as_str), Some("https://amer.ng.msg"));
        assert!(!gtms.contains_key("ignored"), "non-string gtms entries are dropped");
    }
}
