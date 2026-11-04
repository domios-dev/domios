use super::*;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use tokio::sync::oneshot;

struct Request {
    root: PathBuf,
    backend: String,
    identity: Option<Identity>,
    reply: oneshot::Sender<Result<(), String>>,
}

pub(crate) struct IdentityWriter(Result<SyncSender<Request>, String>);

impl Default for IdentityWriter {
    fn default() -> Self {
        let (sender, receiver) = sync_channel::<Request>(16);
        let started = std::thread::Builder::new()
            .name("account-identity-store".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let result = remember(&request.root, &request.backend, request.identity);
                    let _ = request.reply.send(result);
                }
            });
        Self(started.map(|_| sender).map_err(|error| error.to_string()))
    }
}

impl IdentityWriter {
    pub(crate) async fn remember(
        &self,
        root: PathBuf,
        backend: String,
        identity: Option<Identity>,
    ) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        self.0
            .as_ref()
            .map_err(Clone::clone)?
            .try_send(Request {
                root,
                backend,
                identity,
                reply,
            })
            .map_err(|_| "Account identity persistence is unavailable".to_owned())?;
        result
            .await
            .map_err(|_| "Account identity persistence stopped".to_owned())?
    }
}
