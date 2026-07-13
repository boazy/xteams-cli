//! Interactive OAuth 2.0 device-code sign-in: request a code, show it to the user on
//! stderr (the one place an auth prompt writes — stdout stays clean for `-j`), and
//! poll until they authenticate. Returns the FOCI family refresh token.

use std::io::Write as _;
use std::time::{Duration, Instant};

use eyre::Result;
use reqwest::Client;
use tokio::time::sleep;

use super::oauth::{self, DEVICE_CODE_GRANT, DeviceCodeResponse, FOCI_CLIENT, PollOutcome};
use crate::error::OAuthError;

const LOGIN_RESOURCE: &str = "https://graph.microsoft.com";
const SLOW_DOWN_STEP: u64 = 5;

pub async fn login(http: &Client, tenant: &str) -> Result<String> {
    let flow = request_code(http, tenant).await?;
    announce(&flow);
    poll(http, tenant, &flow).await
}

async fn request_code(http: &Client, tenant: &str) -> Result<DeviceCodeResponse> {
    let resp = http
        .post(oauth::devicecode_url(tenant))
        .form(&[
            ("client_id", FOCI_CLIENT),
            ("scope", oauth::scope_for(LOGIN_RESOURCE).as_str()),
        ])
        .send()
        .await?;
    let status = resp.status();
    let body = resp.bytes().await?;
    if !status.is_success() {
        return Err(OAuthError::DeviceCodeRequest {
            status: status.as_u16(),
            body: String::from_utf8_lossy(&body).chars().take(200).collect(),
        }
        .into());
    }
    Ok(oauth::parse_device_code(&body)?)
}

fn announce(flow: &DeviceCodeResponse) {
    let mut stderr = std::io::stderr();
    let _ = writeln!(
        stderr,
        "\nTo sign in, open {} and enter code: {}\nWaiting for sign-in (Ctrl-C to cancel)…",
        flow.verification_uri, flow.user_code
    );
}

async fn poll(http: &Client, tenant: &str, flow: &DeviceCodeResponse) -> Result<String> {
    let mut interval = flow.interval;
    let deadline = Instant::now() + Duration::from_secs(flow.expires_in);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(OAuthError::Timeout.into());
        }
        sleep(Duration::from_secs(interval).min(remaining)).await;
        if Instant::now() >= deadline {
            return Err(OAuthError::Timeout.into());
        }
        let resp = http
            .post(oauth::token_url(tenant))
            .form(&[
                ("grant_type", DEVICE_CODE_GRANT),
                ("client_id", FOCI_CLIENT),
                ("device_code", flow.device_code.as_str()),
            ])
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await?;
        match oauth::classify_poll(status, &body)? {
            PollOutcome::Complete(tokens) => {
                return tokens
                    .refresh_token
                    .ok_or_else(|| OAuthError::MissingField("refresh_token").into());
            }
            PollOutcome::Pending => {}
            PollOutcome::SlowDown => interval += SLOW_DOWN_STEP,
        }
    }
}
