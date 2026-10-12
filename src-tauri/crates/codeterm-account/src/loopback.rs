use crate::{Account, Callback, Error, Result};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub struct Loopback {
    listener: TcpListener,
}
impl Loopback {
    pub async fn bind() -> Result<Self> {
        Ok(Self {
            listener: TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .map_err(|_| Error::Transport)?,
        })
    }
    pub fn redirect(&self) -> Result<String> {
        Ok(format!(
            "http://127.0.0.1:{}/callback",
            self.listener
                .local_addr()
                .map_err(|_| Error::Transport)?
                .port()
        ))
    }
    pub(crate) async fn serve(self, account: Arc<Account>) -> Result<()> {
        loop {
            let (mut stream, _) = self.listener.accept().await.map_err(|_| Error::Transport)?;
            let outcome = tokio::time::timeout(Duration::from_secs(5), async {
                let mut bytes = Vec::new();
                loop {
                    let byte = stream.read_u8().await.map_err(|_| Error::InvalidCallback)?;
                    bytes.push(byte);
                    if bytes.len() > 8192 {
                        return Err(Error::InvalidCallback);
                    }
                    if bytes.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let callback = parse_request(&bytes, &self.redirect()?)?;
                account.prepare_callback(callback).await
            })
            .await
            .unwrap_or(Err(Error::Timeout));
            let accepted = outcome.is_ok();
            let (status, body) = if accepted {
                ("200 OK", "<!doctype html><title>Domios</title><p>Callback received. You can close this tab and return to Domios.</p>")
            } else {
                (
                    "400 Bad Request",
                    "<!doctype html><title>Domios</title><p>Sign-in callback refused.</p>",
                )
            };
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            let _ = tokio::time::timeout(
                Duration::from_secs(2),
                stream.write_all(response.as_bytes()),
            )
            .await;
            if let Ok(completion) = outcome {
                return completion.send();
            }
        }
    }
}

fn parse_request(bytes: &[u8], redirect: &str) -> Result<Callback> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidCallback)?;
    let mut parts = text
        .lines()
        .next()
        .ok_or(Error::InvalidCallback)?
        .split_whitespace();
    if parts.next() != Some("GET") {
        return Err(Error::InvalidCallback);
    }
    let target = parts.next().ok_or(Error::InvalidCallback)?;
    if !target.starts_with("/callback?")
        || parts.next() != Some("HTTP/1.1")
        || parts.next().is_some()
    {
        return Err(Error::InvalidCallback);
    }
    Callback::parse(&format!(
        "{}{}",
        redirect.trim_end_matches("/callback"),
        target
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_request_requires_get_exact_path_and_single_parameters() {
        let request = format!(
            "GET /callback?code={}&state=fixture-state-value HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            "c".repeat(43)
        );
        let redirect = "http://127.0.0.1:43210/callback";
        assert!(parse_request(request.as_bytes(), redirect).is_ok());
        for invalid in [
            request.replace("GET ", "POST "),
            request.replace("/callback?", "/other?"),
            request.replace("&state=", "&code=duplicate&state="),
            request.replace("GET /", "GET https://external.invalid/"),
        ] {
            assert!(parse_request(invalid.as_bytes(), redirect).is_err());
        }
    }
}
