use super::{valid_endpoint, IngressError, MAX_ENROLLMENT_FRAME_BYTES};
use codeterm_util::network_host::public_ip;
use reqwest::{header, Url};
use std::net::{SocketAddr, SocketAddrV4};
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_tungstenite::{
    client_async_with_config,
    tungstenite::{
        handshake::{client::generate_key, derive_accept_key},
        protocol::{Role, WebSocketConfig},
    },
    WebSocketStream,
};

pub enum CloudRoute<'a> {
    Direct(SocketAddrV4),
    Preview {
        origin: &'a Url,
        name: &'a header::HeaderName,
        value: &'a header::HeaderValue,
    },
}

pub(super) trait ChannelIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> ChannelIo for T {}
pub(super) type ClientSocket = WebSocketStream<Box<dyn ChannelIo>>;

pub(super) async fn open(route: CloudRoute<'_>, path: &str) -> Result<ClientSocket, IngressError> {
    let config = WebSocketConfig::default()
        .max_frame_size(Some(MAX_ENROLLMENT_FRAME_BYTES))
        .max_message_size(Some(MAX_ENROLLMENT_FRAME_BYTES));
    match route {
        CloudRoute::Direct(address) => {
            if address.port() != 7687 || !public_ip((*address.ip()).into()) {
                return Err(IngressError::Rejected);
            }
            let stream: Box<dyn ChannelIo> = Box::new(
                TcpStream::connect(address)
                    .await
                    .map_err(|_| IngressError::Unavailable)?,
            );
            client_async_with_config(format!("ws://{address}{path}"), stream, Some(config))
                .await
                .map(|(socket, _)| socket)
                .map_err(|_| IngressError::Unavailable)
        }
        CloudRoute::Preview {
            origin,
            name,
            value,
        } => {
            let canonical = origin.origin().ascii_serialization();
            if origin.as_str() != format!("{canonical}/")
                || !valid_endpoint(&canonical, name.as_str())
                || !value.is_sensitive()
            {
                return Err(IngressError::Rejected);
            }
            let host = origin.domain().ok_or(IngressError::Rejected)?;
            let port = origin
                .port_or_known_default()
                .ok_or(IngressError::Rejected)?;
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| IngressError::Unavailable)?
                .take(33)
                .collect();
            if addresses.is_empty()
                || addresses.len() > 32
                || !addresses.iter().all(|addr| public_ip(addr.ip()))
            {
                return Err(IngressError::Rejected);
            }
            let client = reqwest::Client::builder()
                .https_only(true)
                .no_proxy()
                .http1_only()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .connect_timeout(Duration::from_secs(10))
                .resolve_to_addrs(host, &addresses)
                .build()
                .map_err(|_| IngressError::Unavailable)?;
            let key = generate_key();
            let response = client
                .get(format!("{canonical}{path}"))
                .header(header::CONNECTION, "Upgrade")
                .header(header::UPGRADE, "websocket")
                .header(header::SEC_WEBSOCKET_VERSION, "13")
                .header(header::SEC_WEBSOCKET_KEY, &key)
                .header(name, value.clone())
                .send()
                .await
                .map_err(|_| IngressError::Unavailable)?;
            if matches!(response.status().as_u16(), 401 | 403) {
                return Err(IngressError::ProviderAccessStale);
            }
            let headers = response.headers();
            let header = |key| headers.get(key).and_then(|v| v.to_str().ok()).unwrap_or("");
            if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS
                || !header(header::UPGRADE).eq_ignore_ascii_case("websocket")
                || !header(header::CONNECTION)
                    .split(',')
                    .any(|v| v.trim().eq_ignore_ascii_case("upgrade"))
                || header(header::SEC_WEBSOCKET_ACCEPT) != derive_accept_key(key.as_bytes())
                || headers.contains_key(header::SEC_WEBSOCKET_PROTOCOL)
                || headers.contains_key(header::SEC_WEBSOCKET_EXTENSIONS)
            {
                return Err(IngressError::Rejected);
            }
            let stream = response
                .upgrade()
                .await
                .map_err(|_| IngressError::Unavailable)?;
            Ok(WebSocketStream::from_raw_socket(
                Box::new(stream) as Box<dyn ChannelIo>,
                Role::Client,
                Some(config),
            )
            .await)
        }
    }
}
