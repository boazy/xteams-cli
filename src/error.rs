//! Typed boundary errors. Libraries get precise variants (`thiserror`); the
//! binary surfaces them through `eyre`/`color-eyre` with added context.

use thiserror::Error;

/// Failures while extracting credentials from the local Teams install.
#[derive(Debug, Error)]
pub enum CredsError {
    #[error(
        "Teams cookie database not found at {0}\n\
         (is New Teams installed and signed in? pass --cookies to override)"
    )]
    CookieDbMissing(String),

    #[error("required cookie '{0}' was not found or could not be decrypted")]
    CookieMissing(&'static str),

    #[error(
        "Keychain read failed: {0}\n\
         (approve the 'Microsoft Teams Safe Storage' prompt, or verify the item exists)"
    )]
    Keychain(String),
}

/// Failures while turning cookies into a usable API session.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("could not extract an AAD bearer token from the 'authtoken' cookie")]
    BearerMissing,

    #[error("Teams authz endpoint returned HTTP {status}: {body}")]
    Authz { status: u16, body: String },

    #[error("authz response did not include a skype token")]
    NoSkypeToken,

    #[error("authz response did not include a chat-service (region) endpoint")]
    NoChatService,
}

/// Failures from the internal Teams HTTP APIs.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Teams API [{endpoint}] returned HTTP {status}: {body}")]
    Http {
        endpoint: String,
        status: u16,
        body: String,
    },
}

/// Failures acquiring or minting AAD tokens (device-code + refresh grants).
#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("device-code request failed: HTTP {status}: {body}")]
    DeviceCodeRequest { status: u16, body: String },

    #[error("sign-in was declined in the browser")]
    AuthorizationDeclined,

    #[error("the device code expired before sign-in completed; re-run to get a new code")]
    DeviceCodeExpired,

    #[error("sign-in did not complete within the allotted time")]
    Timeout,

    #[error("token endpoint returned HTTP {status}: {error}: {description}")]
    TokenEndpoint { status: u16, error: String, description: String },

    #[error("token response was missing the '{0}' field")]
    MissingField(&'static str),

    #[error("not signed in for this feature — run `xteams auth login` first")]
    NotLoggedIn,

    #[error("your sign-in expired or was revoked — run `xteams auth login` to sign in again")]
    SessionExpired,
}

/// Failures reading/writing the refresh token in the macOS Keychain.
#[derive(Debug, Error)]
pub enum TokenStoreError {
    #[error("Keychain write failed: {0}")]
    Write(String),

    #[error("Keychain read failed: {0}")]
    Read(String),

    #[error("Keychain delete failed: {0}")]
    Delete(String),
}
