use crate::{transport, types::PeerAddress};

pub(crate) async fn send<T: serde::Serialize + Sync>(addresses: &[PeerAddress], port: u16, path: &str, token: &str, notice: &T) -> Result<(), String> {
    let http = transport::shared_http_client();
    tokio::time::timeout(std::time::Duration::from_secs(3), transport::try_peer(addresses, |address| {
        let request = http.post(format!("{}{path}", transport::peer_http_origin(&address.ip, port)))
            .header("Authorization", format!("Bearer {token}"))
            .header(crate::version::VERSION_HEADER, crate::version::local_version())
            .json(notice);
        async move {
            match request.send().await {
                Ok(response) if response.status().is_success() => transport::TryNext::Success(()),
                Ok(_) => transport::TryNext::Stop("Signed notice refused".into()),
                Err(_) => transport::TryNext::Retry("Peer unreachable".into()),
            }
        }
    })).await.map_err(|_| "Signed notice deadline exceeded".to_string())?
}
