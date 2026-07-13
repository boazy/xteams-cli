//! Pure model of the on-disk token cache: the FRT, per-audience access tokens, the
//! derived skype session, and identity — each with an absolute expiry. No I/O and no
//! clock here (callers pass `now`), so validity/invalidation logic is unit-testable.
//! Disk read/write lives in `token_cache_io`; the clock is `jwt::now_unix`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::credential::CachedCredential;
use super::jwt::Identity;

/// Everything persisted between `xteams` invocations so a valid cache needs no network.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenCache {
    /// The FOCI family refresh token (opaque; may rotate on redemption).
    pub refresh_token: String,
    #[serde(default)]
    pub identity: Identity,
    /// Per-audience access tokens, keyed by resource URL.
    #[serde(default)]
    pub access_tokens: BTreeMap<String, StoredAccessToken>,
    /// The skype session (skypetoken + region + gtms) derived via spaces→authz.
    #[serde(default)]
    pub skype: Option<StoredSkypeSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAccessToken {
    pub token: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSkypeSession {
    pub skype_token: String,
    pub region: String,
    pub chat_service: String,
    #[serde(default)]
    pub gtms: BTreeMap<String, String>,
    pub expires_at: i64,
}

impl TokenCache {
    pub fn new(refresh_token: String) -> Self {
        Self {
            refresh_token,
            ..Default::default()
        }
    }

    /// A cached access token for `resource` that is still valid at `now` with `skew`.
    pub fn valid_access_token(&self, resource: &str, now: i64, skew: i64) -> Option<&str> {
        self.access_tokens
            .get(resource)
            .filter(|t| now + skew < t.expires_at)
            .map(|t| t.token.as_str())
    }

    pub fn set_access_token(&mut self, resource: &str, token: String, expires_at: i64) {
        self.access_tokens
            .insert(resource.to_owned(), StoredAccessToken { token, expires_at });
    }

    /// The cached skype session if still valid at `now` with `skew`.
    pub fn valid_skype(&self, now: i64, skew: i64) -> Option<&StoredSkypeSession> {
        self.skype.as_ref().filter(|s| now + skew < s.expires_at)
    }

    pub fn set_skype(&mut self, session: StoredSkypeSession) {
        self.skype = Some(session);
    }

    pub fn set_refresh_token(&mut self, token: String) {
        self.refresh_token = token;
    }

    /// Evict exactly the credential a 401 rejected, so the next run re-mints it. A
    /// rejected spaces token also poisons any skype session derived from it.
    pub fn invalidate(&mut self, credential: &CachedCredential) {
        match credential {
            CachedCredential::AccessToken { resource } => {
                self.access_tokens.remove(resource);
                if resource == super::SPACES_RESOURCE {
                    self.skype = None;
                }
            }
            CachedCredential::SkypeSession => self.skype = None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn cache() -> TokenCache {
        let mut c = TokenCache::new("frt-0".to_owned());
        c.set_access_token("https://graph.microsoft.com", "graph-tok".to_owned(), 1_000);
        c.set_skype(StoredSkypeSession {
            skype_token: "skype".to_owned(),
            region: "amer".to_owned(),
            chat_service: "https://amer.example".to_owned(),
            gtms: BTreeMap::new(),
            expires_at: 1_000,
        });
        c
    }

    #[test]
    fn access_token_validity_respects_skew() {
        let c = cache();
        let res = "https://graph.microsoft.com";
        assert_eq!(c.valid_access_token(res, 800, 60), Some("graph-tok"));
        // 940 + 60 == 1000, not strictly less -> treated as expired.
        assert_eq!(c.valid_access_token(res, 940, 60), None);
        assert_eq!(c.valid_access_token(res, 2_000, 60), None);
        assert_eq!(c.valid_access_token("https://unknown", 0, 60), None);
    }

    #[test]
    fn skype_validity_respects_skew() {
        let c = cache();
        assert!(c.valid_skype(800, 60).is_some());
        assert!(c.valid_skype(999, 60).is_none());
    }

    #[test]
    fn invalidate_access_token_removes_only_that_resource() {
        let mut c = cache();
        c.set_access_token("https://substrate.office.com", "sub".to_owned(), 5_000);
        c.invalidate(&CachedCredential::access("https://graph.microsoft.com"));
        assert!(
            c.valid_access_token("https://graph.microsoft.com", 0, 0)
                .is_none()
        );
        assert_eq!(
            c.valid_access_token("https://substrate.office.com", 0, 0),
            Some("sub")
        );
        assert!(
            c.skype.is_some(),
            "unrelated invalidation must not drop skype"
        );
    }

    #[test]
    fn invalidate_spaces_token_also_drops_skype() {
        let mut c = cache();
        c.set_access_token(super::super::SPACES_RESOURCE, "spaces".to_owned(), 5_000);
        c.invalidate(&CachedCredential::access(super::super::SPACES_RESOURCE));
        assert!(
            c.skype.is_none(),
            "a rejected spaces token poisons the derived skype session"
        );
    }

    #[test]
    fn invalidate_skype_session_drops_only_skype() {
        let mut c = cache();
        c.invalidate(&CachedCredential::SkypeSession);
        assert!(c.skype.is_none());
        assert_eq!(
            c.valid_access_token("https://graph.microsoft.com", 0, 0),
            Some("graph-tok")
        );
    }

    #[test]
    fn round_trips_through_json() {
        let json = serde_json::to_string(&cache()).expect("serialize");
        let back: TokenCache = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.refresh_token, "frt-0");
        assert_eq!(
            back.valid_access_token("https://graph.microsoft.com", 0, 0),
            Some("graph-tok")
        );
    }
}
