//! Typed boundary errors. Libraries get precise variants (`thiserror`); the
//! binary surfaces them through `eyre`/`color-eyre` with added context.

use thiserror::Error;

use crate::auth::CachedCredential;

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

    #[cfg(target_os = "macos")]
    #[error(
        "Keychain read failed: {0}\n\
         (approve the 'Microsoft Teams Safe Storage' prompt, or verify the item exists)"
    )]
    Keychain(String),

    #[cfg(windows)]
    #[error("Windows credential extraction failed: {0}")]
    Windows(String),

    #[cfg(not(any(target_os = "macos", windows)))]
    #[error(
        "cookie extraction from the local Teams app is not supported on this platform\n\
         (run `xteams auth login` to sign in with a device code — it needs no cookies)"
    )]
    UnsupportedPlatform,
}

/// Failures while turning cookies (or an FRT-derived spaces token) into a session.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("could not extract an AAD bearer token from the 'authtoken' cookie")]
    BearerMissing,

    #[error("Teams authz endpoint returned HTTP {status}: {body}")]
    Authz { status: u16, body: String },

    #[error(
        "Teams authz rejected the cached credential {credential:?} (HTTP 401): {body}"
    )]
    AuthzUnauthorized { credential: CachedCredential, body: String },

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

    #[error(
        "Teams API [{endpoint}] returned HTTP 401 for cached credential {credential:?}: {body}"
    )]
    Unauthorized {
        endpoint: String,
        credential: CachedCredential,
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

/// Failures reading/writing the on-disk token cache and its refresh lock.
#[derive(Debug, Error)]
pub enum TokenStoreError {
    #[error("could not resolve a token-store directory (check $HOME / $XDG_STATE_HOME)")]
    StoreDir,

    #[error("failed to read the token cache at {path}: {detail}")]
    Read { path: String, detail: String },

    #[error("failed to write the token cache at {path}: {detail}")]
    Write { path: String, detail: String },

    #[error("the token cache at {path} is corrupt ({detail}) — run `xteams auth login` to rebuild it")]
    Corrupt { path: String, detail: String },

    #[error(
        "another xteams process is refreshing credentials\n\
         (lock {path} held since {since}, {age}); re-run once it finishes, or delete the lock"
    )]
    LockHeld { path: String, since: String, age: String },

    #[error("could not manage the refresh lock at {path}: {detail}")]
    Lock { path: String, detail: String },
}

/// Failures while seeding another CLI's credential store from xteams' tokens.
#[derive(Debug, Error)]
pub enum SeedError {
    #[error("could not resolve your home directory")]
    HomeDir,

    #[error("failed to serialize {what}: {detail}")]
    Serialize { what: &'static str, detail: String },

    #[error("failed to write {path}: {detail}")]
    Write { path: String, detail: String },

    #[error("the Graph token is missing oid/tid claims — run `xteams auth login` first")]
    NoIdentity,
}

/// Failures while converting message content between formats.
#[derive(Debug, Error)]
pub enum ContentError {
    #[error("unknown content format '{0}' (expected plain, html, markdown, keep, or pandoc:<fmt>)")]
    UnknownFormat(String),

    #[error("'keep' is only valid as an output format, not an input format")]
    KeepAsInput,

    #[error("--pandoc-{0} is reserved (xteams controls the pandoc conversion direction)")]
    ReservedPandocOption(String),

    #[error("could not run pandoc (is it installed and on PATH?): {0}")]
    PandocSpawn(#[source] std::io::Error),

    #[error("pandoc ({from} -> {to}) exited with {status}: {stderr}")]
    Pandoc { from: String, to: String, status: String, stderr: String },

    #[error("HTML-to-markdown conversion failed: {0}")]
    HtmlToMarkdown(#[source] std::io::Error),

    #[error("HTML-to-text conversion failed: {0}")]
    HtmlToText(String),
}
