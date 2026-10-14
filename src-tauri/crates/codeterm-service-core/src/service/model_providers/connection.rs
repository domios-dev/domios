use super::ModelProviderError;
use crate::secrets::SecretString;
use codeterm_plugin::model_providers::{EndpointKind, ModelProviderDefinition};
use std::{io::Read, time::Duration};

pub(super) fn validate_url(raw: &str) -> Result<url::Url, ModelProviderError> {
    let url = url::Url::parse(raw).map_err(|_| ModelProviderError::InvalidRequest)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ModelProviderError::InvalidRequest);
    }
    Ok(url)
}

pub(super) fn test(
    provider: &ModelProviderDefinition,
    key: &SecretString,
) -> Result<Vec<String>, ModelProviderError> {
    test_with_deadline(provider, key, Duration::from_secs(10))
}

fn test_with_deadline(
    provider: &ModelProviderDefinition,
    key: &SecretString,
    deadline: Duration,
) -> Result<Vec<String>, ModelProviderError> {
    let mut url = validate_url(&provider.base_url)?;
    let base = url.path().trim_end_matches('/');
    let path = if provider.kind == EndpointKind::Anthropic && !base.ends_with("/v1") {
        format!("{base}/v1/models")
    } else {
        format!("{base}/models")
    };
    url.set_path(&path);
    let client = reqwest::blocking::Client::builder()
        .timeout(deadline)
        .connect_timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ModelProviderError::Connection)?;
    let request = client.get(url);
    let request = match provider.kind {
        EndpointKind::Anthropic => request
            .header("x-api-key", key.expose_secret())
            .header("anthropic-version", "2023-06-01"),
        EndpointKind::OpenaiCompatible => request.bearer_auth(key.expose_secret()),
    };
    let response = request.send().map_err(|e| {
        if e.is_timeout() {
            ModelProviderError::Timeout
        } else {
            ModelProviderError::Connection
        }
    })?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err(ModelProviderError::Unauthorized);
    }
    if !response.status().is_success() {
        return Err(ModelProviderError::Connection);
    }
    let mut bytes = Vec::new();
    response
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                ModelProviderError::Timeout
            } else {
                ModelProviderError::Connection
            }
        })?;
    if bytes.len() > 1_048_576 {
        return Err(ModelProviderError::InvalidResponse);
    }
    parse_models(&bytes)
}

fn parse_models(bytes: &[u8]) -> Result<Vec<String>, ModelProviderError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| ModelProviderError::InvalidResponse)?;
    let data = value
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or(ModelProviderError::InvalidResponse)?;
    let mut models = data
        .iter()
        .map(|row| {
            row.get("id")
                .and_then(|v| v.as_str())
                .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or(ModelProviderError::InvalidResponse)
        })
        .collect::<Result<Vec<_>, _>>()?;
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err(ModelProviderError::InvalidResponse);
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn provider(base_url: String) -> ModelProviderDefinition {
        ModelProviderDefinition {
            id: "demo".into(),
            kind: EndpointKind::OpenaiCompatible,
            base_url,
            models: vec![],
            key_env: None,
            name: None,
            secret_name: None,
        }
    }

    #[test]
    fn model_provider_real_check_authenticates_and_returns_models() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", server.local_addr().unwrap());
        let key = ["opaque", "network", "fixture"].join("-");
        let expected = key.clone();
        let serving = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut lines = vec![];
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                lines.push(line);
            }
            assert_eq!(lines[0], "GET /v1/models HTTP/1.1\r\n");
            assert!(lines
                .iter()
                .any(|line| line.trim() == format!("authorization: Bearer {expected}")));
            let body = r#"{"data":[{"id":"local-model"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        assert_eq!(
            test(&provider(endpoint), &SecretString::from(key)).unwrap(),
            vec!["local-model"]
        );
        serving.join().unwrap();
    }

    #[test]
    fn model_provider_real_check_times_out_without_polling_or_retry() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.local_addr().unwrap());
        let (release, wait) = std::sync::mpsc::channel();
        let serving = std::thread::spawn(move || {
            let (stream, _) = server.accept().unwrap();
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            drop(stream);
        });
        let result = test_with_deadline(
            &provider(endpoint),
            &SecretString::from("opaque"),
            Duration::from_millis(100),
        );
        release.send(()).unwrap();
        serving.join().unwrap();
        assert_eq!(result.unwrap_err(), ModelProviderError::Timeout);
    }
    #[test]
    fn model_provider_model_response_is_typed_and_deduplicated() {
        assert_eq!(
            parse_models(br#"{"data":[{"id":"b"},{"id":"a"},{"id":"b"}]}"#).unwrap(),
            vec!["a", "b"]
        );
        for bytes in [
            br#"{"data":[]}"#.as_slice(),
            br#"{"data":[{"name":"a"}]}"#,
            b"invalid",
        ] {
            assert_eq!(
                parse_models(bytes).unwrap_err(),
                ModelProviderError::InvalidResponse
            );
        }
    }
    #[test]
    fn model_provider_urls_cannot_carry_credentials() {
        for url in [
            "file:///tmp/a",
            "https://user:password@example.com",
            "https://example.com?key=a",
            "https://example.com#key",
        ] {
            assert_eq!(
                validate_url(url).unwrap_err(),
                ModelProviderError::InvalidRequest
            );
        }
        assert!(validate_url("http://127.0.0.1:1234/v1").is_ok());
    }
}
