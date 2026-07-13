//! Credential identity tags: which cached secret authenticated a request, so a 401
//! can be traced back to the exact cache entry to invalidate. Shared by `api`,
//! `auth`, and `error` (`ApiError::Unauthorized` / `AuthError::AuthzUnauthorized`).

use crate::error::{ApiError, AuthError};

/// A specific secret held in the on-disk token cache. Doubles as the cache key space
/// (access tokens are keyed by resource URL) and as the invalidation target when a
/// request authenticated by it is rejected with HTTP 401.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CachedCredential {
    /// A per-audience OAuth access token, keyed by its resource URL.
    AccessToken { resource: String },
    /// The derived skype session (skypetoken + region + gtms) for the chat service.
    SkypeSession,
}

impl CachedCredential {
    pub fn access(resource: impl Into<String>) -> Self {
        Self::AccessToken {
            resource: resource.into(),
        }
    }
}

/// How an `ApiClient` session authenticated. The FRT-derived skype session can be
/// invalidated (it lives in the cache); desktop cookies cannot (nothing is cached).
#[derive(Debug, Clone)]
pub enum SessionCredential {
    CachedSkype,
    Cookie,
}

impl SessionCredential {
    /// The cache entry to evict if the chat service rejects this session, if any.
    pub fn cached_credential(&self) -> Option<CachedCredential> {
        match self {
            Self::CachedSkype => Some(CachedCredential::SkypeSession),
            Self::Cookie => None,
        }
    }
}

/// Walk an error chain for a rejected cached credential — an API 401
/// (`ApiError::Unauthorized`) or an authz 401 on a cached spaces token
/// (`AuthError::AuthzUnauthorized`). The top-level handler evicts what this returns.
pub fn credential_to_invalidate(report: &eyre::Report) -> Option<CachedCredential> {
    for cause in report.chain() {
        if let Some(ApiError::Unauthorized { credential, .. }) = cause.downcast_ref::<ApiError>() {
            return Some(credential.clone());
        }
        if let Some(AuthError::AuthzUnauthorized { credential, .. }) =
            cause.downcast_ref::<AuthError>()
        {
            return Some(credential.clone());
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use eyre::WrapErr as _;

    #[test]
    fn cached_skype_maps_to_skype_session() {
        assert_eq!(
            SessionCredential::CachedSkype.cached_credential(),
            Some(CachedCredential::SkypeSession)
        );
        assert_eq!(SessionCredential::Cookie.cached_credential(), None);
    }

    #[test]
    fn finds_api_unauthorized_even_when_wrapped() {
        let report = Err::<(), _>(ApiError::Unauthorized {
            endpoint: "GET conversations".to_owned(),
            credential: CachedCredential::SkypeSession,
            body: "expired".to_owned(),
        })
        .wrap_err("while listing chats")
        .expect_err("must be an error");
        assert_eq!(
            credential_to_invalidate(&report),
            Some(CachedCredential::SkypeSession)
        );
    }

    #[test]
    fn finds_authz_unauthorized_spaces_access_token() {
        let spaces = "https://api.spaces.skype.com";
        let report: eyre::Report = AuthError::AuthzUnauthorized {
            credential: CachedCredential::access(spaces),
            body: "bad token".to_owned(),
        }
        .into();
        assert_eq!(
            credential_to_invalidate(&report),
            Some(CachedCredential::access(spaces))
        );
    }

    #[test]
    fn unrelated_error_yields_none() {
        let report = eyre::eyre!("network down");
        assert_eq!(credential_to_invalidate(&report), None);
    }
}
