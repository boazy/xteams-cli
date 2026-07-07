//! Shared HTTP client for the internal Teams APIs.

use std::path::Path;

use eyre::Result;
use reqwest::Method;

use crate::auth::{self, Session};
use crate::error::ApiError;

pub mod chat;

const AUTH_HEADER: &str = "Authentication";

#[derive(Debug)]
pub struct ApiClient {
    http: reqwest::Client,
    session: Session,
}

impl ApiClient {
    pub async fn connect(cookies: Option<&Path>) -> Result<Self> {
        let (http, session) = auth::connect(cookies).await?;
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
        let resp = request.send().await?;
        let status = resp.status();
        if status.is_success() {
            Ok(resp)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(ApiError::Http {
                endpoint: endpoint.to_owned(),
                status: status.as_u16(),
                body: body.chars().take(240).collect(),
            }
            .into())
        }
    }
}
