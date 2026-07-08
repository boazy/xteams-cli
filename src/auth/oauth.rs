//! Pure OAuth 2.0 device-code + refresh-grant decision logic: response parsing,
//! poll-error classification, and access-token cache expiry. No I/O lives here —
//! the network calls are in `device_code.rs` and the refresh grant in `auth.rs`;
//! this is the testable core. Consumers (`team`/`user`/`calendar`) are gated on the
//! live PoC proof (docs/oneauth-handoff.md §B); the `dead_code` allowance is removed
//! once those commands consume this module.
#![allow(dead_code)]

use serde::Deserialize;

use crate::error::OAuthError;

pub const FOCI_CLIENT: &str = "1fec8e78-bce4-4aaf-ab1b-5451cc387264";
pub const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_TTL_SECS: i64 = 3600;

const fn default_interval() -> u64 {
    5
}

const fn default_expiry() -> u64 {
    900
}

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default = "default_expiry")]
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub foci: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorBody {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Next action after one device-code token poll.
#[derive(Debug)]
pub enum PollOutcome {
    Complete(Box<TokenResponse>),
    Pending,
    SlowDown,
}

pub fn scope_for(resource: &str) -> String {
    format!("{resource}/.default offline_access")
}

pub fn parse_device_code(body: &[u8]) -> Result<DeviceCodeResponse, OAuthError> {
    serde_json::from_slice(body).map_err(|_| OAuthError::MissingField("device_code"))
}

/// Classify a device-code token-poll response into the next action or a terminal error.
pub fn classify_poll(status: u16, body: &[u8]) -> Result<PollOutcome, OAuthError> {
    if status == 200 {
        let tokens: TokenResponse =
            serde_json::from_slice(body).map_err(|_| OAuthError::MissingField("access_token"))?;
        return Ok(PollOutcome::Complete(Box::new(tokens)));
    }
    let parsed: Result<OAuthErrorBody, _> = serde_json::from_slice(body);
    let Ok(err) = parsed else {
        return Err(OAuthError::TokenEndpoint {
            status,
            error: "unparseable_error".to_owned(),
            description: String::new(),
        });
    };
    match err.error.as_str() {
        "authorization_pending" => Ok(PollOutcome::Pending),
        "slow_down" => Ok(PollOutcome::SlowDown),
        "authorization_declined" => Err(OAuthError::AuthorizationDeclined),
        "expired_token" | "code_expired" => Err(OAuthError::DeviceCodeExpired),
        other => Err(OAuthError::TokenEndpoint {
            status,
            error: other.to_owned(),
            description: err.error_description.unwrap_or_default(),
        }),
    }
}

/// Parse a refresh-grant / device-code token response body.
pub fn parse_token(body: &[u8]) -> Result<TokenResponse, OAuthError> {
    serde_json::from_slice(body).map_err(|_| OAuthError::MissingField("access_token"))
}

/// A minted access token with an absolute expiry, for the per-audience cache.
#[derive(Debug, Clone)]
pub struct CachedToken {
    pub value: String,
    pub expires_at_unix: i64,
}

impl CachedToken {
    pub fn from_response(resp: &TokenResponse, now_unix: i64) -> Self {
        let ttl = resp.expires_in.unwrap_or(DEFAULT_TTL_SECS);
        Self { value: resp.access_token.clone(), expires_at_unix: now_unix + ttl }
    }

    pub fn is_valid(&self, now_unix: i64, skew_secs: i64) -> bool {
        now_unix + skew_secs < self.expires_at_unix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PENDING: &[u8] = br#"{"error":"authorization_pending","error_description":"AADSTS70016: OAuth 2.0 device flow error. Authorization is pending."}"#;
    const TOKENS: &[u8] = br#"{"access_token":"eyJhbGciOiJnb29kIn0.body.sig","refresh_token":"0.rt","expires_in":3599,"scope":"https://chatsvcagg.teams.microsoft.com/.default","foci":"1"}"#;

    #[test]
    fn classify_poll_returns_complete_on_200() {
        let outcome = classify_poll(200, TOKENS).expect("200 body should parse");
        match outcome {
            PollOutcome::Complete(tokens) => {
                assert_eq!(tokens.refresh_token.as_deref(), Some("0.rt"));
                assert_eq!(tokens.foci.as_deref(), Some("1"));
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn classify_poll_maps_pending_and_slow_down() {
        assert!(matches!(classify_poll(400, PENDING), Ok(PollOutcome::Pending)));
        let slow = br#"{"error":"slow_down"}"#;
        assert!(matches!(classify_poll(400, slow), Ok(PollOutcome::SlowDown)));
    }

    #[test]
    fn classify_poll_maps_terminal_errors() {
        let declined = br#"{"error":"authorization_declined"}"#;
        assert!(matches!(classify_poll(400, declined), Err(OAuthError::AuthorizationDeclined)));
        let expired = br#"{"error":"expired_token"}"#;
        assert!(matches!(classify_poll(400, expired), Err(OAuthError::DeviceCodeExpired)));
        let other = br#"{"error":"invalid_grant","error_description":"bad"}"#;
        assert!(matches!(classify_poll(400, other), Err(OAuthError::TokenEndpoint { .. })));
    }

    #[test]
    fn parse_device_code_reads_fields_and_defaults() {
        let body = br#"{"device_code":"dc","user_code":"ABC","verification_uri":"https://login.microsoft.com/device"}"#;
        let parsed = parse_device_code(body).expect("device code should parse");
        assert_eq!(parsed.user_code, "ABC");
        assert_eq!(parsed.interval, 5);
        assert_eq!(parsed.expires_in, 900);
    }

    #[test]
    fn cached_token_expiry_respects_skew() {
        let resp = parse_token(TOKENS).expect("token should parse");
        let token = CachedToken::from_response(&resp, 1_000);
        assert_eq!(token.expires_at_unix, 1_000 + 3599);
        assert!(token.is_valid(1_000, 60));
        assert!(!token.is_valid(4_600, 60));
        assert!(!token.is_valid(4_550, 60));
    }

    #[test]
    fn scope_for_appends_default_and_offline_access() {
        assert_eq!(
            scope_for("https://substrate.office.com"),
            "https://substrate.office.com/.default offline_access"
        );
    }
}
