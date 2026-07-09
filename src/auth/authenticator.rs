//! FRT-backed token provider over the on-disk cache. Reads the cache lock-free on the
//! all-valid fast path; every mutation (refresh rotation, access-token/skype-session
//! save, invalidation) runs under `CacheLock` with a reload+double-check so concurrent
//! `xteams` runs never clobber a rotated FRT or resurrect a stale token.

use eyre::Result;
use reqwest::{Client, Method, RequestBuilder};

use super::credential::CachedCredential;
use super::jwt::{Identity, identity_from_jwt, jwt_expiry, now_unix};
use super::lock::{AuthInteraction, CacheLock};
use super::oauth::{self, FOCI_CLIENT};
use super::session::{self, AuthzResponse};
use super::token_cache::{StoredSkypeSession, TokenCache};
use super::{SPACES_RESOURCE, token_cache_io};
use crate::error::{AuthError, OAuthError};

const EXPIRY_SKEW_SECS: i64 = 60;
const DEFAULT_TTL_SECS: i64 = 3600;
const FALLBACK_SKYPE_TTL_SECS: i64 = 45 * 60;

/// Per-audience bearer tokens (and the skype session) backed by one stored FRT.
pub struct Authenticator {
    http: Client,
    tenant: String,
    interaction: AuthInteraction,
}

impl Authenticator {
    /// Load an authenticator; errors `NotLoggedIn` when no FRT is stored.
    pub fn load(http: Client, tenant: &str, interaction: AuthInteraction) -> Result<Self> {
        match Self::try_load(http, tenant, interaction)? {
            Some(auth) => Ok(auth),
            None => Err(OAuthError::NotLoggedIn.into()),
        }
    }

    /// `Some` iff a non-empty FRT is stored (used by the FRT-first `connect`).
    pub fn try_load(http: Client, tenant: &str, interaction: AuthInteraction) -> Result<Option<Self>> {
        let has_frt = token_cache_io::load()?.map(|c| !c.refresh_token.is_empty()).unwrap_or(false);
        Ok(has_frt.then(|| Self::new(http, tenant, interaction)))
    }

    /// Device-code sign-in; writes a fresh cache holding only the new FRT.
    pub async fn login(http: Client, tenant: &str, interaction: AuthInteraction) -> Result<Self> {
        let refresh = super::device_code::login(&http, tenant).await?;
        let _lock = CacheLock::acquire(interaction).await?;
        token_cache_io::save(&TokenCache::new(refresh))?;
        Ok(Self::new(http, tenant, interaction))
    }

    fn new(http: Client, tenant: &str, interaction: AuthInteraction) -> Self {
        Self { http, tenant: tenant.to_owned(), interaction }
    }

    pub async fn authed(&self, resource: &str, method: Method, url: &str) -> Result<RequestBuilder> {
        let token = self.token_for(resource).await?;
        Ok(self.http.request(method, url).bearer_auth(token))
    }

    /// The FRT (for seeding other CLIs); errors `NotLoggedIn` when absent/empty.
    pub fn refresh_token(&self) -> Result<String> {
        let frt = token_cache_io::load()?.map(|c| c.refresh_token).unwrap_or_default();
        if frt.is_empty() {
            return Err(OAuthError::NotLoggedIn.into());
        }
        Ok(frt)
    }

    /// The cached identity (populated opportunistically from minted-token claims).
    pub fn identity(&self) -> Result<Identity> {
        Ok(token_cache_io::load()?.map(|c| c.identity).unwrap_or_default())
    }

    pub async fn region(&self) -> Result<String> {
        Ok(self.skype_session().await?.region)
    }

    /// A per-audience access token: cached if valid, else minted under the lock.
    pub async fn token_for(&self, resource: &str) -> Result<String> {
        let now = now_unix();
        if let Some(cache) = token_cache_io::load()?
            && let Some(token) = cache.valid_access_token(resource, now, EXPIRY_SKEW_SECS)
        {
            return Ok(token.to_owned());
        }
        let _lock = CacheLock::acquire(self.interaction).await?;
        let mut cache = token_cache_io::load()?.ok_or(OAuthError::NotLoggedIn)?;
        let now = now_unix();
        if let Some(token) = cache.valid_access_token(resource, now, EXPIRY_SKEW_SECS) {
            return Ok(token.to_owned());
        }
        let refresh = cache.refresh_token.clone();
        if refresh.is_empty() {
            return Err(OAuthError::NotLoggedIn.into());
        }
        let response = self.redeem(&refresh, resource).await?;
        if let Some(rotated) = response.refresh_token.clone() {
            cache.set_refresh_token(rotated);
        }
        let expires_at = now_unix() + response.expires_in.unwrap_or(DEFAULT_TTL_SECS);
        cache.set_access_token(resource, response.access_token.clone(), expires_at);
        set_identity_if_empty(&mut cache, &response.access_token);
        token_cache_io::save(&cache)?;
        Ok(response.access_token)
    }

    /// The skype session for the chat service: cached if valid, else derived from a
    /// (cached or freshly minted) spaces token via authz, and persisted.
    pub async fn skype_session(&self) -> Result<StoredSkypeSession> {
        let now = now_unix();
        if let Some(cache) = token_cache_io::load()?
            && let Some(skype) = cache.valid_skype(now, EXPIRY_SKEW_SECS)
        {
            return Ok(skype.clone());
        }
        let spaces = self.token_for(SPACES_RESOURCE).await?;
        let _lock = CacheLock::acquire(self.interaction).await?;
        let mut cache = token_cache_io::load()?.ok_or(OAuthError::NotLoggedIn)?;
        if let Some(skype) = cache.valid_skype(now_unix(), EXPIRY_SKEW_SECS) {
            return Ok(skype.clone());
        }
        let credential = CachedCredential::access(SPACES_RESOURCE);
        let parsed: AuthzResponse = session::post_authz(&self.http, &spaces, Some(credential)).await?;
        let skype_token = parsed.skype_token().ok_or(AuthError::NoSkypeToken)?;
        let gtms = parsed.gtms();
        let chat_service = gtms.get("chatService").cloned().ok_or(AuthError::NoChatService)?;
        let stored = StoredSkypeSession {
            region: parsed.region.clone().unwrap_or_default(),
            expires_at: skype_expiry(&skype_token, &spaces, now_unix()),
            skype_token,
            chat_service,
            gtms,
        };
        cache.set_skype(stored.clone());
        token_cache_io::save(&cache)?;
        Ok(stored)
    }

    async fn redeem(&self, refresh: &str, resource: &str) -> Result<oauth::TokenResponse> {
        let resp = self
            .http
            .post(oauth::token_url(&self.tenant))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", FOCI_CLIENT),
                ("refresh_token", refresh),
                ("scope", oauth::scope_for(resource).as_str()),
            ])
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await?;
        if status != 200 {
            let code = oauth::error_code(&body);
            if code.as_deref() == Some("invalid_grant") {
                // The FRT is dead: clear the whole cache so the next run signs in clean.
                token_cache_io::delete()?;
                return Err(OAuthError::SessionExpired.into());
            }
            return Err(OAuthError::TokenEndpoint {
                status,
                error: code.unwrap_or_else(|| "refresh_grant_failed".to_owned()),
                description: String::from_utf8_lossy(&body).chars().take(200).collect(),
            }
            .into());
        }
        Ok(oauth::parse_token(&body)?)
    }
}

/// Clear exactly the credential a 401 rejected (top-level handler), under the lock.
pub async fn invalidate_credential(
    credential: &CachedCredential,
    interaction: AuthInteraction,
) -> Result<()> {
    let _lock = CacheLock::acquire(interaction).await?;
    if let Some(mut cache) = token_cache_io::load()? {
        cache.invalidate(credential);
        token_cache_io::save(&cache)?;
    }
    Ok(())
}

pub fn logout() -> Result<()> {
    token_cache_io::delete()?;
    Ok(())
}

/// Skype-session expiry precedence: min(skypeToken exp, spaces exp), else spaces exp,
/// else the skypeToken exp, else a conservative fallback TTL.
fn skype_expiry(skype_token: &str, spaces_token: &str, now: i64) -> i64 {
    match (jwt_expiry(skype_token), jwt_expiry(spaces_token)) {
        (Some(skype), Some(spaces)) => skype.min(spaces),
        (Some(skype), None) => skype,
        (None, Some(spaces)) => spaces,
        (None, None) => now + FALLBACK_SKYPE_TTL_SECS,
    }
}

fn set_identity_if_empty(cache: &mut TokenCache, token: &str) {
    let id = &cache.identity;
    if id.upn.is_none() && id.name.is_none() && id.tenant.is_none() {
        let decoded = identity_from_jwt(token);
        if decoded.upn.is_some() || decoded.name.is_some() || decoded.tenant.is_some() {
            cache.identity = decoded;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt_with_exp(exp: i64) -> String {
        use base64::Engine as _;
        let payload = serde_json::json!({ "exp": exp }).to_string();
        let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        format!("ey.{body}.sig")
    }

    #[test]
    fn skype_expiry_caps_to_the_earlier_of_skype_and_spaces() {
        let skype = jwt_with_exp(2_000);
        let spaces = jwt_with_exp(1_500);
        assert_eq!(skype_expiry(&skype, &spaces, 0), 1_500);
        assert_eq!(skype_expiry(&skype, "opaque", 0), 2_000);
        assert_eq!(skype_expiry("opaque", &spaces, 0), 1_500);
        assert_eq!(skype_expiry("opaque", "opaque", 100), 100 + FALLBACK_SKYPE_TTL_SECS);
    }

    #[test]
    fn set_identity_only_fills_when_empty() {
        use base64::Engine as _;
        let claims = serde_json::json!({ "upn": "u@c.com", "tid": "t" }).to_string();
        let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims);
        let token = format!("ey.{body}.sig");

        let mut cache = TokenCache::new("frt".to_owned());
        set_identity_if_empty(&mut cache, &token);
        assert_eq!(cache.identity.upn.as_deref(), Some("u@c.com"));

        // A second, different token must not overwrite an already-populated identity.
        let other = serde_json::json!({ "upn": "other@c.com" }).to_string();
        let other = format!("ey.{}.sig", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(other));
        set_identity_if_empty(&mut cache, &other);
        assert_eq!(cache.identity.upn.as_deref(), Some("u@c.com"));
    }
}
