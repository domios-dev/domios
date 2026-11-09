use super::*;
use std::sync::mpsc::{sync_channel, SyncSender};
use tokio::sync::oneshot;

enum Request {
    Read(oneshot::Sender<io::Result<AccountMeshRecord>>),
    Write(AccountMeshRecord, oneshot::Sender<io::Result<()>>),
}

pub(super) struct FileOwner(Result<SyncSender<Request>, String>);

impl FileOwner {
    pub(super) fn new(path: PathBuf) -> Self {
        Self::start(path, || {})
    }

    fn start(path: PathBuf, mut before_write: impl FnMut() + Send + 'static) -> Self {
        let (sender, receiver) = sync_channel::<Request>(16);
        let started = std::thread::Builder::new()
            .name("account-mesh-store".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Read(reply) => {
                            let _ = reply.send(read(&path));
                        }
                        Request::Write(record, reply) => {
                            before_write();
                            let _ = reply.send(write(&path, &record));
                        }
                    }
                }
            });
        Self(started.map(|_| sender).map_err(|error| error.to_string()))
    }

    fn send(&self, request: Request) -> io::Result<()> {
        self.0
            .as_ref()
            .map_err(|error| io::Error::other(error.clone()))?
            .try_send(request)
            .map_err(|error| io::Error::new(io::ErrorKind::WouldBlock, error.to_string()))
    }

    pub(super) async fn load(&self) -> io::Result<AccountMeshRecord> {
        let (reply, result) = oneshot::channel();
        self.send(Request::Read(reply))?;
        result.await.map_err(io::Error::other)?
    }

    pub(super) async fn save(&self, record: AccountMeshRecord) -> io::Result<()> {
        let (reply, result) = oneshot::channel();
        self.send(Request::Write(record, reply))?;
        result.await.map_err(io::Error::other)?
    }
}

#[cfg(test)]
mod tests;
