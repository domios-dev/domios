use super::*;
use tokio::sync::{mpsc, OnceCell};
use tokio::task::AbortHandle;

const CAPACITY: usize = 16;

enum CredentialAction {
    Load,
    Save(u64, Secret),
    Delete(u64),
}

type CredentialRequest = (CredentialAction, oneshot::Sender<Result<Option<Secret>>>);

pub(super) struct CredentialOwner {
    worker: OnceCell<(mpsc::Sender<CredentialRequest>, AbortHandle)>,
}

impl CredentialOwner {
    pub(super) fn new() -> Self {
        Self {
            worker: OnceCell::new(),
        }
    }

    async fn request(&self, account: &Account, action: CredentialAction) -> Result<Option<Secret>> {
        let (sender, _) = self
            .worker
            .get_or_init(|| async {
                let (sender, mut receiver) = mpsc::channel::<CredentialRequest>(CAPACITY);
                let store = account.store.clone();
                let provider = account.provider.id().to_owned();
                let state = Arc::downgrade(&account.state);
                let task = tokio::spawn(async move {
                    let mut saved_generation = 0;
                    while let Some((action, reply)) = receiver.recv().await {
                        let Some(state) = state.upgrade() else {
                            break;
                        };
                        let result = match action {
                            CredentialAction::Load => store.load(&provider).await,
                            CredentialAction::Save(generation, token) => {
                                let result =
                                    save(&store, &provider, &state, generation, token).await;
                                if result.is_ok() {
                                    saved_generation = generation;
                                }
                                result
                            }
                            CredentialAction::Delete(generation) => {
                                let superseded =
                                    state.lock().await.session.as_ref().is_some_and(|session| {
                                        session.login_generation > generation
                                    });
                                if superseded || saved_generation > generation {
                                    Err(Error::Cancelled)
                                } else {
                                    store.delete(&provider).await.map(|()| None)
                                }
                            }
                        };
                        let _ = reply.send(result);
                    }
                });
                (sender, task.abort_handle())
            })
            .await;
        let (reply, result) = oneshot::channel();
        sender
            .try_send((action, reply))
            .map_err(|_| Error::Pending)?;
        tokio::time::timeout(account.deadline, result)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Cancelled)?
    }

    pub(super) async fn load(&self, account: &Account) -> Result<Option<Secret>> {
        self.request(account, CredentialAction::Load).await
    }

    pub(super) async fn save(
        &self,
        account: &Account,
        generation: u64,
        token: Secret,
    ) -> Result<()> {
        self.request(account, CredentialAction::Save(generation, token))
            .await
            .map(|_| ())
    }

    pub(super) async fn delete(&self, account: &Account, generation: u64) -> Result<()> {
        self.request(account, CredentialAction::Delete(generation))
            .await
            .map(|_| ())
    }
}

async fn save(
    store: &Arc<dyn CredentialStore>,
    provider: &str,
    state: &Mutex<State>,
    generation: u64,
    token: Secret,
) -> Result<Option<Secret>> {
    if state.lock().await.generation != generation {
        return Err(Error::Cancelled);
    }
    let saved = store.load(provider).await?;
    if state.lock().await.generation != generation {
        return Err(Error::Cancelled);
    }
    if saved.as_ref().map(Secret::expose) != Some(token.expose()) {
        store.save(provider, &token).await?;
    }
    let session_only = store.session_only(provider).await;
    let mut state = state.lock().await;
    if state.generation != generation {
        return Err(Error::Cancelled);
    }
    state.session_only = session_only;
    Ok(None)
}

impl Drop for CredentialOwner {
    fn drop(&mut self) {
        if let Some((_, task)) = self.worker.get() {
            task.abort();
        }
    }
}

struct Notification {
    revision: u64,
    identity: Option<Identity>,
    announce: bool,
    refreshed: Option<Session>,
    reply: oneshot::Sender<Result<()>>,
}

#[derive(Default)]
struct Notifications {
    pending: parking_lot::Mutex<Option<Notification>>,
    ready: Notify,
}

pub(super) struct ObserverOwner {
    worker: OnceCell<(Arc<Notifications>, AbortHandle)>,
}

impl ObserverOwner {
    pub(super) fn new() -> Self {
        Self {
            worker: OnceCell::new(),
        }
    }

    pub(super) async fn notify(
        &self,
        account: &Account,
        revision: u64,
        identity: Option<Identity>,
        announce: bool,
        refreshed: Option<Session>,
    ) -> Result<()> {
        let Some(observer) = account.observer.clone() else {
            return Ok(());
        };
        let (mailbox, _) = self
            .worker
            .get_or_init(|| async {
                let mailbox = Arc::new(Notifications::default());
                let inbox = mailbox.clone();
                let state = Arc::downgrade(&account.state);
                let deadline = account.deadline;
                let task = tokio::spawn(async move {
                    let mut last_identity: Option<Option<Identity>> = None;
                    loop {
                        let notified = inbox.ready.notified();
                        let notification = inbox.pending.lock().take();
                        let Some(notification) = notification else {
                            notified.await;
                            continue;
                        };
                        let Notification {
                            revision,
                            identity,
                            announce,
                            refreshed,
                            reply,
                        } = notification;
                        let Some(state) = state.upgrade() else {
                            break;
                        };
                        let announce = announce || last_identity.as_ref() != Some(&identity);
                        let result = async {
                            if state.lock().await.publication_revision != revision {
                                return Err(Error::Cancelled);
                            }
                            if announce {
                                observer.changed(identity.as_ref()).await;
                            }
                            if state.lock().await.publication_revision != revision {
                                return Err(Error::Cancelled);
                            }
                            if let Some(session) = &refreshed {
                                observer.session_refreshed(session).await;
                            }
                            if state.lock().await.publication_revision != revision {
                                return Err(Error::Cancelled);
                            }
                            Ok(())
                        };
                        let result = tokio::time::timeout(deadline, result);
                        tokio::pin!(result);
                        let result = loop {
                            let changed = inbox.ready.notified();
                            if inbox.pending.lock().as_ref().is_some_and(|next| next.revision > revision) {
                                break Err(Error::Cancelled);
                            }
                            tokio::select! {
                                outcome = &mut result => break outcome.unwrap_or(Err(Error::Timeout)),
                                _ = changed => {}
                            }
                        };
                        if result.is_ok() { last_identity = Some(identity.clone()); }
                        let _ = reply.send(result);
                    }
                });
                (mailbox, task.abort_handle())
            })
            .await;
        let (reply, result) = oneshot::channel();
        {
            let mut pending = mailbox.pending.lock();
            if pending
                .as_ref()
                .is_some_and(|current| current.revision > revision)
            {
                return Err(Error::Cancelled);
            }
            if let Some(previous) = pending.replace(Notification {
                revision,
                identity,
                announce,
                refreshed,
                reply,
            }) {
                let _ = previous.reply.send(Err(Error::Cancelled));
            }
        }
        mailbox.ready.notify_one();
        tokio::time::timeout(account.deadline, result)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Cancelled)?
    }
}

impl Drop for ObserverOwner {
    fn drop(&mut self) {
        if let Some((_, task)) = self.worker.get() {
            task.abort();
        }
    }
}
