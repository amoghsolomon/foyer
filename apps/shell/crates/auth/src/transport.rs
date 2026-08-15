use std::env;
use std::fmt::{Debug, Formatter};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client as HttpClient, Method, StatusCode};
use serde_json::Value;

use crate::error::{AuthError, RequestError};

#[derive(Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub path: String,
    pub bearer: Option<String>,
    pub json: Option<Value>,
}

impl Debug for HttpRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("bearer", &self.bearer.as_ref().map(|_| "[redacted]"))
            .field("json", &self.json.as_ref().map(|_| "[omitted]"))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: StatusCode,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn exchange(&self, request: HttpRequest) -> Result<HttpResponse, RequestError>;
}

#[derive(Clone)]
pub struct ReqwestTransport {
    http: HttpClient,
    base_url: String,
}

impl ReqwestTransport {
    pub fn from_env() -> Result<Self, AuthError> {
        let http = HttpClient::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| AuthError::Transport(format!("build HTTP client: {error}")))?;
        Ok(Self {
            http,
            base_url: api_base_url(),
        })
    }

    pub fn new(http: HttpClient, base_url: impl Into<String>) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }
}

pub fn api_base_url() -> String {
    env::var("FOYER_API_BASE_URL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:3583".into())
        .trim_end_matches('/')
        .to_string()
}

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn exchange(&self, request: HttpRequest) -> Result<HttpResponse, RequestError> {
        let url = format!("{}{}", self.base_url, request.path);
        let mut builder = self.http.request(request.method, url);
        if let Some(token) = request.bearer.as_deref() {
            builder = builder.bearer_auth(token);
        }
        if let Some(body) = request.json {
            builder = builder.json(&body);
        }
        let response = builder.send().await?;
        let status = response.status();
        let body = response.bytes().await?.to_vec();
        Ok(HttpResponse { status, body })
    }
}
