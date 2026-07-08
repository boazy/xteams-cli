//! Minted-token provider: holds the FOCI family refresh token and mints per-audience
//! access tokens on demand, cached until near expiry. Region is discovered lazily via
//! the spaces→authz path, so bearer features need no cookies.

use std::collections::HashMap;
use std::sync::Mutex;

use eyre::{Result, eyre};
use reqwest::{Client, Method, RequestBuilder};
use serde::Deserialize;

use super::device_code;
use super::oauth::{self, CachedToken, FOCI_CLIENT};
use super::store;
use crate::error::OAuthError;

const SPACES_RESOURCE: &str = "https://api.spaces.skype.com";
const AUTHZ_URL: &str = "https://authsvc.teams.microsoft.com/v1.0/authz";
const EXPIRY_SKEW_SECS: i64 = 60;

/// A source of per-audience bearer tokens backed by one stored FOCI refresh token.
pub struct Authenticator {
    http: Client,
    tenant: String,
    refresh_token: Mutex<String>,
    cache: Mutex<HashMap<String, CachedToken>>,
    region: Mutex<Option<String>>,
}

impl Authenticator {
    pub async fn load(http: Client, tenant: &str) -> Result<Self> {
        let refresh = store::load_refresh_token()?.ok_or(OAuthError::NotLoggedIn)?;
        Ok(Self::new(http, tenant, refresh))
    }

    pub async fn login(http: Client, tenant: &str) -> Result<Self> {
        let refresh = device_code::login(&http, tenant).await?;
        store::save_refresh_token(&refresh)?;
        Ok(Self::new(http, tenant, refresh))
    }

    fn new(http: Client, tenant: &str, refresh: String) -> Self {
        Self {
            http,
            tenant: tenant.to_owned(),
            refresh_token: Mutex::new(refresh),
            cache: Mutex::new(HashMap::new()),
            region: Mutex::new(None),
        }
    }

    pub async fn authed(&self, resource: &str, method: Method, url: &str) -> Result<RequestBuilder> {
        let token = self.token_for(resource).await?;
        Ok(self.http.request(method, url).bearer_auth(token))
    }

    pub fn refresh_token(&self) -> Result<String> {
        Ok(self.refresh_token.lock().map_err(lock_err)?.clone())
    }

    pub async fn token_for(&self, resource: &str) -> Result<String> {
        if let Some(token) = self.cached(resource)? {
            return Ok(token);
        }
        let response = self.redeem(resource).await?;
        if let Some(rotated) = response.refresh_token.clone() {
            *self.refresh_token.lock().map_err(lock_err)? = rotated.clone();
            store::save_refresh_token(&rotated)?;
        }
        let token = CachedToken::from_response(&response, now_unix());
        let value = token.value.clone();
        self.cache.lock().map_err(lock_err)?.insert(resource.to_owned(), token);
        Ok(value)
    }

    fn cached(&self, resource: &str) -> Result<Option<String>> {
        let cache = self.cache.lock().map_err(lock_err)?;
        Ok(cache
            .get(resource)
            .filter(|token| token.is_valid(now_unix(), EXPIRY_SKEW_SECS))
            .map(|token| token.value.clone()))
    }

    async fn redeem(&self, resource: &str) -> Result<oauth::TokenResponse> {
        let refresh = self.refresh_token.lock().map_err(lock_err)?.clone();
        let resp = self
            .http
            .post(oauth::token_url(&self.tenant))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", FOCI_CLIENT),
                ("refresh_token", refresh.as_str()),
                ("scope", oauth::scope_for(resource).as_str()),
            ])
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await?;
        if status != 200 {
            return Err(OAuthError::TokenEndpoint {
                status,
                error: "refresh_grant_failed".to_owned(),
                description: String::from_utf8_lossy(&body).chars().take(200).collect(),
            }
            .into());
        }
        Ok(oauth::parse_token(&body)?)
    }

    pub async fn region(&self) -> Result<String> {
        let cached = self.region.lock().map_err(lock_err)?.clone();
        if let Some(region) = cached {
            return Ok(region);
        }
        let token = self.token_for(SPACES_RESOURCE).await?;
        let resp = self
            .http
            .post(AUTHZ_URL)
            .bearer_auth(token)
            .json(&serde_json::json!({}))
            .send()
            .await?;
        let parsed: AuthzRegion = resp.error_for_status()?.json().await?;
        let region = parsed.region.ok_or_else(|| eyre!("authz response had no region"))?;
        *self.region.lock().map_err(lock_err)? = Some(region.clone());
        Ok(region)
    }
}

pub fn logout() -> Result<()> {
    store::delete_refresh_token()?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct AuthzRegion {
    region: Option<String>,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn lock_err<T>(_: std::sync::PoisonError<T>) -> eyre::Report {
    eyre!("internal token-cache lock poisoned")
}
