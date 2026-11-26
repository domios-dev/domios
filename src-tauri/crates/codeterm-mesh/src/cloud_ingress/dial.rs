use super::{IngressError, MAX_ENROLLMENT_FRAME_BYTES};
use codeterm_util::cloud_endpoint::{CloudEndpoint, PROVIDER_PROXY_PORT};
use rustls_pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::{
    client_async_with_config, tungstenite::protocol::WebSocketConfig, MaybeTlsStream,
    WebSocketStream,
};

pub type CloudRoute = CloudEndpoint;

pub(crate) use codeterm_util::cloud_endpoint::{
    direct_address_allowed, provider_proxy_host_allowed,
};

pub(super) type ClientStream = MaybeTlsStream<TcpStream>;
pub(super) type ClientSocket = WebSocketStream<ClientStream>;

#[cfg(test)]
pub(super) struct TestTlsOverride {
    pub target: std::net::SocketAddr,
    pub roots: Arc<rustls::RootCertStore>,
}

#[cfg(test)]
tokio::task_local! {
    pub(super) static TEST_TLS_OVERRIDE: TestTlsOverride;
}

fn build_tls_config(
    roots: Arc<rustls::RootCertStore>,
) -> Result<Arc<rustls::ClientConfig>, IngressError> {
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| IngressError::TlsFailed)?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

fn webpki_tls_config() -> Result<Arc<rustls::ClientConfig>, IngressError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    build_tls_config(Arc::new(roots))
}

async fn open_provider_proxy(
    host: &str,
    path: &str,
    config: WebSocketConfig,
) -> Result<ClientSocket, IngressError> {
    if !provider_proxy_host_allowed(host) {
        return Err(IngressError::Rejected);
    }
    let server_name = match ServerName::try_from(host.to_owned()) {
        Ok(name @ ServerName::DnsName(_)) => name,
        _ => return Err(IngressError::Rejected),
    };
    #[cfg(test)]
    let (tcp_target, tls_config) =
        match TEST_TLS_OVERRIDE.try_with(|ov| (ov.target, Arc::clone(&ov.roots))) {
            Ok((target, roots)) => (Some(target), build_tls_config(roots)?),
            Err(_) => (None, webpki_tls_config()?),
        };
    #[cfg(not(test))]
    let (tcp_target, tls_config): (Option<std::net::SocketAddr>, Arc<rustls::ClientConfig>) =
        (None, webpki_tls_config()?);

    let tcp = match tcp_target {
        Some(target) => TcpStream::connect(target).await,
        None => TcpStream::connect((host, PROVIDER_PROXY_PORT)).await,
    }
    .map_err(|_| IngressError::Unavailable)?;

    let tls = TlsConnector::from(tls_config)
        .connect(server_name, tcp)
        .await
        .map_err(|_| IngressError::TlsFailed)?;

    client_async_with_config(
        format!("wss://{host}{path}"),
        MaybeTlsStream::Rustls(tls),
        Some(config),
    )
    .await
    .map(|(socket, _)| socket)
    .map_err(|_| IngressError::Unavailable)
}

pub(super) async fn open(route: &CloudRoute, path: &str) -> Result<ClientSocket, IngressError> {
    let config = WebSocketConfig::default()
        .max_frame_size(Some(MAX_ENROLLMENT_FRAME_BYTES))
        .max_message_size(Some(MAX_ENROLLMENT_FRAME_BYTES));
    match route {
        CloudRoute::Direct(address) => {
            if !direct_address_allowed(*address) {
                return Err(IngressError::Rejected);
            }
            let stream = TcpStream::connect(*address)
                .await
                .map_err(|_| IngressError::Unavailable)?;
            client_async_with_config(
                format!("ws://{address}{path}"),
                MaybeTlsStream::Plain(stream),
                Some(config),
            )
            .await
            .map(|(socket, _)| socket)
            .map_err(|_| IngressError::Unavailable)
        }
        CloudRoute::ProviderProxy { host } => open_provider_proxy(host, path, config).await,
    }
}
