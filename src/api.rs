//! Shared HTTP client for the internal Teams APIs.

use std::path::Path;

use eyre::Result;
use reqwest::Method;

use crate::auth::{self, AuthInteraction, CachedCredential, Session};
use crate::error::ApiError;

pub mod calendar;
pub mod chat;
pub mod csa;
pub mod substrate;

const AUTH_HEADER: &str = "Authentication";

#[derive(Debug)]
pub struct ApiClient {
    http: reqwest::Client,
    session: Session,
}

impl ApiClient {
    pub async fn connect(cookies: Option<&Path>, interaction: AuthInteraction) -> Result<Self> {
        let (http, session) = auth::connect(cookies, interaction).await?;
        Ok(Self { http, session })
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    fn chat(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}/v1/users/ME/{path}", self.session.chat_service);
        self.http
            .request(method, url)
            .header(AUTH_HEADER, format!("skypetoken={}", self.session.skype_token))
    }

    async fn exec(&self, request: reqwest::RequestBuilder, endpoint: &str) -> Result<reqwest::Response> {
        send_ok(request, endpoint, self.session.credential.cached_credential()).await
    }
}

/// Send a request and map any non-2xx to `ApiError`. A 401 carrying a known cached
/// credential becomes `ApiError::Unauthorized` so the top-level handler can evict
/// exactly that token; otherwise it maps to `ApiError::Http`. Shared by the skypetoken
/// chat path and the bearer-token (chatsvcagg/substrate/graph) paths.
pub(crate) async fn send_ok(
    request: reqwest::RequestBuilder,
    endpoint: &str,
    credential: Option<CachedCredential>,
) -> Result<reqwest::Response> {
    let resp = request.send().await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body: String = resp.text().await.unwrap_or_default().chars().take(240).collect();
    if status == reqwest::StatusCode::UNAUTHORIZED
        && let Some(credential) = credential
    {
        return Err(ApiError::Unauthorized { endpoint: endpoint.to_owned(), credential, body }.into());
    }
    Err(ApiError::Http { endpoint: endpoint.to_owned(), status: status.as_u16(), body }.into())
}
