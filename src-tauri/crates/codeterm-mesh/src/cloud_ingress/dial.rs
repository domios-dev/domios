use super::{IngressError, MAX_ENROLLMENT_FRAME_BYTES};
use codeterm_util::cloud_endpoint::{CloudEndpoint, PROVIDER_PROXY_PORT};
use codeterm_util::network_host::public_ip;
use rustls_pki_types::ServerName;
use std::net::SocketAddr;
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
pub(super) type TestResolverFn =
    Arc<dyn Fn(&str, u16) -> Result<Vec<SocketAddr>, IngressError> + Send + Sync>;

#[cfg(test)]
pub(super) struct TestTlsOverride {
    pub target: SocketAddr,
    pub roots: Arc<rustls::RootCertStore>,
}

#[cfg(test)]
tokio::task_local! {
    pub(super) static TEST_TLS_OVERRIDE: TestTlsOverride;
    pub(super) static TEST_RESOLVER: TestResolverFn;
}

pub(super) fn vet_resolved_provider_proxy_addrs(
    addrs: &[SocketAddr],
) -> Result<Vec<SocketAddr>, IngressError> {
    if addrs.is_empty() {
        return Err(IngressError::Unavailable);
    }
    if addrs
        .iter()
        .any(|addr| addr.port() != PROVIDER_PROXY_PORT || !public_ip(addr.ip()))
    {
        return Err(IngressError::Rejected);
    }
    Ok(addrs.to_vec())
}

async fn resolve_provider_proxy_addrs(host: &str) -> Result<Vec<SocketAddr>, IngressError> {
    #[cfg(test)]
    if let Ok(resolver) = TEST_RESOLVER.try_with(Arc::clone) {
        return resolver(host, PROVIDER_PROXY_PORT);
    }
    let resolved = tokio::net::lookup_host((host, PROVIDER_PROXY_PORT))
        .await
        .map_err(|_| IngressError::Unavailable)?;
    Ok(resolved.collect())
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
    let (test_target, tls_config) =
        match TEST_TLS_OVERRIDE.try_with(|ov| (ov.target, Arc::clone(&ov.roots))) {
            Ok((target, roots)) => (Some(target), build_tls_config(roots)?),
            Err(_) => (None, webpki_tls_config()?),
        };
    #[cfg(not(test))]
    let (test_target, tls_config): (Option<SocketAddr>, Arc<rustls::ClientConfig>) =
        (None, webpki_tls_config()?);

    let connect_targets = match test_target {
        Some(target) => {
            #[cfg(test)]
            if TEST_RESOLVER.try_with(|_| ()).is_ok() {
                let resolved = resolve_provider_proxy_addrs(host).await?;
                vet_resolved_provider_proxy_addrs(&resolved)?;
            }
            vec![target]
        }
        None => {
            let resolved = resolve_provider_proxy_addrs(host).await?;
            vet_resolved_provider_proxy_addrs(&resolved)?
        }
    };

    let mut tcp = None;
    for target in connect_targets {
        if let Ok(stream) = TcpStream::connect(target).await {
            tcp = Some(stream);
            break;
        }
    }
    let tcp = tcp.ok_or(IngressError::Unavailable)?;

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
