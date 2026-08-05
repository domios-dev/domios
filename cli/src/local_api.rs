use std::sync::{Arc, Mutex};

use reqwest::{header::HeaderMap, StatusCode};
use serde::{de::DeserializeOwned, Serialize};

use crate::client::{discover_socket, SocketClient};

#[derive(Clone)]
pub struct LocalApiClient {
    client: Arc<Mutex<SocketClient>>,
    headers: HeaderMap,
}

impl LocalApiClient {
    pub fn connect() -> Result<Self, LocalApiError> {
        let socket = discover_socket().map_err(LocalApiError::new)?;
        let client = SocketClient::connect(&socket).map_err(LocalApiError::new)?;
        let mut headers = HeaderMap::new();
        for (env, header) in [
            ("CODETERM_TAB_ID", "x-codeterm-tab-id"),
            ("CODETERM_SESSION_ID", "x-codeterm-session-id"),
            ("CODETERM_SESSION_BINDING_NONCE", "x-codeterm-binding-nonce"),
            ("CODETERM_WORKER_ID", "x-codeterm-worker-id"),
        ] {
            if let Ok(value) = std::env::var(env) {
                if let Ok(value) = value.parse() {
                    headers.insert(header, value);
                }
            }
        }
        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            headers,
        })
    }

    pub fn get(&self, url: impl AsRef<str>) -> LocalApiRequest {
        self.request("GET", url)
    }

    pub fn post(&self, url: impl AsRef<str>) -> LocalApiRequest {
        self.request("POST", url)
    }

    pub fn put(&self, url: impl AsRef<str>) -> LocalApiRequest {
        self.request("PUT", url)
    }

    pub fn patch(&self, url: impl AsRef<str>) -> LocalApiRequest {
        self.request("PATCH", url)
    }

    pub fn delete(&self, url: impl AsRef<str>) -> LocalApiRequest {
        self.request("DELETE", url)
    }

    fn request(&self, method: &str, url: impl AsRef<str>) -> LocalApiRequest {
        LocalApiRequest {
            client: self.clone(),
            method: method.to_owned(),
            path: local_path(url.as_ref()),
            headers: self.headers.clone(),
            body: None,
        }
    }
}

pub struct LocalApiRequest {
    client: LocalApiClient,
    method: String,
    path: String,
    headers: HeaderMap,
    body: Option<String>,
}

impl LocalApiRequest {
    pub fn header(
        mut self,
        name: impl reqwest::header::IntoHeaderName,
        value: impl ToString,
    ) -> Self {
        if let Ok(value) = value.to_string().parse() {
            self.headers.insert(name, value);
        }
        self
    }

    pub fn json<T: Serialize + ?Sized>(mut self, value: &T) -> Self {
        self.body = serde_json::to_string(value).ok();
        self.headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        self
    }

    pub fn body(mut self, value: impl Into<String>) -> Self {
        self.body = Some(value.into());
        self
    }

    pub fn timeout(self, _timeout: std::time::Duration) -> Self {
        self
    }

    pub fn send(self) -> Result<LocalApiResponse, LocalApiError> {
        let headers = self
            .headers
            .iter()
            .filter_map(|(name, value)| {
                value.to_str().ok().map(|value| {
                    (
                        name.to_string(),
                        serde_json::Value::String(value.to_owned()),
                    )
                })
            })
            .collect::<serde_json::Map<_, _>>();
        let params = serde_json::json!({
            "method": self.method,
            "path": self.path,
            "headers": headers,
            "body": self.body.unwrap_or_default(),
        });
        let value = self
            .client
            .client
            .lock()
            .map_err(|_| LocalApiError::new("local API client lock poisoned"))?
            .call("api.request", Some(params))
            .map_err(LocalApiError::new)?;
        LocalApiResponse::from_value(value)
    }
}

pub struct LocalApiResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl LocalApiResponse {
    fn from_value(value: serde_json::Value) -> Result<Self, LocalApiError> {
        let status = value
            .get("status")
            .and_then(serde_json::Value::as_u64)
            .and_then(|status| StatusCode::from_u16(status as u16).ok())
            .ok_or_else(|| LocalApiError::new("local API response missing status"))?;
        let mut headers = HeaderMap::new();
        if let Some(values) = value.get("headers").and_then(serde_json::Value::as_object) {
            for (name, value) in values {
                if let (Ok(name), Some(value)) = (
                    reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                    value.as_str(),
                ) {
                    if let Ok(value) = value.parse() {
                        headers.insert(name, value);
                    }
                }
            }
        }
        let body = value
            .get("body")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .as_bytes()
            .to_vec();
        Ok(Self {
            status,
            headers,
            body,
        })
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn text(self) -> Result<String, LocalApiError> {
        String::from_utf8(self.body).map_err(LocalApiError::new)
    }

    pub fn json<T: DeserializeOwned>(self) -> Result<T, LocalApiError> {
        serde_json::from_slice(&self.body).map_err(LocalApiError::new)
    }

    pub fn bytes(self) -> Result<Vec<u8>, LocalApiError> {
        Ok(self.body)
    }

    pub fn error_for_status(self) -> Result<Self, LocalApiError> {
        if self.status.is_success() {
            Ok(self)
        } else {
            Err(LocalApiError::new(format!(
                "HTTP status {}: {}",
                self.status,
                String::from_utf8_lossy(&self.body)
            )))
        }
    }
}

#[derive(Debug)]
pub struct LocalApiError(String);

impl LocalApiError {
    fn new(error: impl ToString) -> Self {
        Self(error.to_string())
    }
}

impl std::fmt::Display for LocalApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LocalApiError {}

fn local_path(url: &str) -> String {
    if url.starts_with("/api/") {
        return url.to_owned();
    }
    url.find("/api/")
        .map(|offset| url[offset..].to_owned())
        .unwrap_or_else(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_path_strips_network_origin() {
        assert_eq!(
            local_path("http://127.0.0.1:7685/api/tabs?agents=true"),
            "/api/tabs?agents=true"
        );
        assert_eq!(local_path("/api/mesh/peers"), "/api/mesh/peers");
    }

    #[test]
    fn response_preserves_status_headers_and_body() {
        let response = LocalApiResponse::from_value(serde_json::json!({
            "status": 429,
            "headers": {"retry-after": "17"},
            "body": "{\"error\":\"busy\"}",
        }))
        .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["retry-after"], "17");
        assert_eq!(response.text().unwrap(), "{\"error\":\"busy\"}");
    }
}
