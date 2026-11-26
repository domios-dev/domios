use super::{IngressError, MAX_ENROLLMENT_FRAME_BYTES};
use std::net::SocketAddrV4;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    client_async_with_config, tungstenite::protocol::WebSocketConfig, WebSocketStream,
};

#[derive(Clone, Copy)]
pub enum CloudRoute {
    Direct(SocketAddrV4),
}

pub(crate) fn direct_address_allowed(address: SocketAddrV4) -> bool {
    address.port() == 7687 && codeterm_util::network_host::public_ip((*address.ip()).into())
}

pub(super) type ClientSocket = WebSocketStream<TcpStream>;

pub(super) async fn open(route: CloudRoute, path: &str) -> Result<ClientSocket, IngressError> {
    let CloudRoute::Direct(address) = route;
    if !direct_address_allowed(address) {
        return Err(IngressError::Rejected);
    }
    let config = WebSocketConfig::default()
        .max_frame_size(Some(MAX_ENROLLMENT_FRAME_BYTES))
        .max_message_size(Some(MAX_ENROLLMENT_FRAME_BYTES));
    let stream = TcpStream::connect(address)
        .await
        .map_err(|_| IngressError::Unavailable)?;
    client_async_with_config(format!("ws://{address}{path}"), stream, Some(config))
        .await
        .map(|(socket, _)| socket)
        .map_err(|_| IngressError::Unavailable)
}
