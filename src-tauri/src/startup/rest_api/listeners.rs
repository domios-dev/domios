use super::*;
use std::future::Future;

type Observer = Arc<dyn Fn(Vec<SocketAddr>) + Send + Sync>;

#[derive(Clone, Default)]
pub(crate) struct ListenerPublication {
    state: Arc<Mutex<PublicationState>>,
    publication: Arc<Mutex<()>>,
}

#[derive(Default)]
struct PublicationState {
    remote_bind: Option<SocketAddr>,
    ready: Vec<SocketAddr>,
    suspended: Vec<SocketAddr>,
    observer: Option<Observer>,
}

impl ListenerPublication {
    pub(crate) fn configure(&self, remote: Option<SocketAddr>, observer: Observer) {
        let mut state = self.state.lock();
        state.remote_bind = remote;
        state.observer = Some(observer);
    }

    pub(crate) fn remote_bind(&self) -> Option<SocketAddr> {
        self.state.lock().remote_bind
    }

    pub(crate) fn ready(&self) -> Vec<SocketAddr> {
        self.state.lock().ready.clone()
    }

    pub(crate) fn publish(&self, ready: Vec<SocketAddr>) {
        self.update_ready(move |state| {
            state.ready = ready;
            state.suspended.clear();
        });
    }

    pub(crate) fn suspend(&self) {
        self.update_ready(|state| {
            if !state.ready.is_empty() {
                state.suspended = std::mem::take(&mut state.ready);
            }
        });
    }

    pub(crate) fn resume(&self) {
        self.update_ready(|state| {
            if !state.suspended.is_empty() {
                state.ready = std::mem::take(&mut state.suspended);
            }
        });
    }

    fn was_proven(&self, address: SocketAddr) -> bool {
        let state = self.state.lock();
        state.ready.contains(&address) || state.suspended.contains(&address)
    }

    fn update_ready(&self, update: impl FnOnce(&mut PublicationState)) {
        let _publication = self.publication.lock();
        let (observer, ready) = {
            let mut state = self.state.lock();
            update(&mut state);
            (state.observer.clone(), state.ready.clone())
        };
        if let Some(observer) = observer {
            observer(ready);
        }
    }

    fn remote_lost(&self, remote: SocketAddr) {
        self.update_ready(|state| {
            state.ready.retain(|address| *address != remote);
            state.suspended.retain(|address| *address != remote);
        });
    }
}

pub(crate) fn additional_bind(
    config: &crate::config::schema::ApiConfig,
    named_dev: bool,
) -> Result<Option<SocketAddr>, String> {
    let Some(host) = &config.mesh_tailnet_host else {
        return Ok(None);
    };
    let ip: std::net::Ipv4Addr = host
        .parse()
        .map_err(|_| "mesh_tailnet_host must be a numeric tailnet IPv4 address")?;
    let octets = ip.octets();
    if !named_dev || octets[0] != 100 || !(64..=127).contains(&octets[1]) {
        return Err(
            "mesh_tailnet_host requires a named development instance and a 100.64.0.0/10 address"
                .into(),
        );
    }
    if !crate::config::canonical_api_bind_addr(config)?
        .ip()
        .is_loopback()
        || config
            .token
            .as_ref()
            .is_none_or(|token| token.trim().is_empty())
    {
        return Err("mesh_tailnet_host requires an authenticated loopback primary listener".into());
    }
    Ok(Some(SocketAddr::new(ip.into(), config.port)))
}

pub(super) async fn bind_remote(
    primary: tokio::net::TcpListener,
    remote: Option<SocketAddr>,
) -> std::io::Result<(tokio::net::TcpListener, Option<tokio::net::TcpListener>)> {
    let remote = match remote {
        Some(address) => {
            let assigned = tokio::task::spawn_blocking(move || {
                if_addrs::get_if_addrs().map(|interfaces| {
                    interfaces
                        .iter()
                        .any(|interface| interface.ip() == address.ip())
                })
            })
            .await
            .map_err(std::io::Error::other)??;
            if !assigned {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    "requested tailnet address is not locally assigned",
                ));
            }
            Some(bind_listener(&address.to_string()).await?)
        }
        None => None,
    };
    Ok((primary, remote))
}

pub(super) async fn serve<P>(
    primary: P,
    remote: Option<tokio::net::TcpListener>,
    router: axum::Router,
    publication: ListenerPublication,
) -> std::io::Result<()>
where
    P: Future<Output = std::io::Result<()>>,
{
    tokio::pin!(primary);
    let Some(remote) = remote else {
        return primary.await;
    };
    let address = remote.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let remote = axum::serve(
        remote,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = shutdown_rx.await;
    })
    .into_future();
    serve_pair(primary, remote, address, publication, Some(shutdown_tx)).await
}

async fn serve_pair<P, R>(
    primary: P,
    remote: R,
    address: SocketAddr,
    publication: ListenerPublication,
    shutdown: Option<oneshot::Sender<()>>,
) -> std::io::Result<()>
where
    P: Future<Output = std::io::Result<()>>,
    R: Future<Output = std::io::Result<()>>,
{
    tokio::pin!(primary);
    tokio::pin!(remote);
    tokio::select! {
        result = &mut primary => {
            publication.publish(Vec::new());
            if let Some(shutdown) = shutdown {
                let _ = shutdown.send(());
                remote.await?;
            }
            result
        },
        result = &mut remote => {
            let had_ready = publication.was_proven(address);
            publication.remote_lost(address);
            log::error!("[api] tailnet listener {address} exited: {result:?}; remote readiness withdrawn");
            if !had_ready {
                return Err(std::io::Error::other("tailnet listener exited before startup readiness"));
            }
            primary.await
        }
    }
}

pub(super) async fn prove_set(
    primary: SocketAddr,
    remote: Option<SocketAddr>,
) -> Result<(), String> {
    prove_health(primary).await?;
    if let Some(remote) = remote {
        prove_health(remote).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
