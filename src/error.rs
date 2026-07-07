//! Typed boundary errors. Libraries get precise variants (`thiserror`); the
//! binary surfaces them through `anyhow` with added context.

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
