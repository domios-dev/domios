use super::lease_model::LeaseModel;
use super::ProfileKind;
use crate::cdp::CdpClient;
use crate::process::{BrowserProcess, CloseOutcome, CLOSE_DEADLINE};
use parking_lot::Mutex;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;
use tokio::sync::{oneshot, Mutex as AsyncMutex};

pub(crate) struct SharedBrowser {
    pub client: CdpClient,
    pub process: Option<Arc<BrowserProcess>>,
    pub binary: Option<PathBuf>,
    pub profile: ProfileKind,
    pub dir: PathBuf,
}

impl SharedBrowser {
    pub fn is_alive(&self) -> bool {
        self.client.close_reason().is_none()
            && self
                .process
                .as_ref()
                .is_none_or(|process| process.exit_status().is_none())
    }

    async fn stop(&self, deadline: Duration, generation: u64) -> Option<CloseOutcome> {
        let close = async {
            let _ = self.client.send("Browser.close", json!({})).await;
        };
        let outcome = match &self.process {
            Some(process) => Some(process.close_with(close, deadline).await),
            None => {
                let _ = tokio::time::timeout(deadline, close).await;
                None
            }
        };
        self.client.close("shared browser stopped");
        let confirmed_exit =
            self.process.is_none() || outcome.is_some_and(CloseOutcome::confirms_exit);
        if !self.profile.is_persistent() || super::instance::is_ephemeral(&self.dir).await {
            if confirmed_exit {
                let _ = tokio::fs::remove_dir_all(&self.dir).await;
            } else if let Some(process) = &self.process {
                let mut exit = process.exit_watch();
                let dir = self.dir.clone();
                let slot = slot_for(&dir);
                tokio::spawn(async move {
                    if exit.wait_for(Option::is_some).await.is_err() {
                        log::warn!(
                            "[browser] profile {} retained without exit confirmation",
                            dir.display()
                        );
                        return;
                    }
                    let model = slot.lock().await;
                    if model.permits_cleanup(generation) {
                        let _ = tokio::fs::remove_dir_all(&dir).await;
                    }
                });
            }
        }
        outcome
    }
}

type Slot = AsyncMutex<LeaseModel<Arc<SharedBrowser>>>;

fn slots() -> &'static Mutex<HashMap<PathBuf, Weak<Slot>>> {
    static SLOTS: OnceLock<Mutex<HashMap<PathBuf, Weak<Slot>>>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn slot_for(dir: &PathBuf) -> Arc<Slot> {
    let key = if crate::sweep::paths_case_insensitive() {
        PathBuf::from(dir.to_string_lossy().to_ascii_lowercase())
    } else {
        dir.clone()
    };
    let mut slots = slots().lock();
    slots.retain(|_, slot| slot.strong_count() > 0);
    if let Some(slot) = slots.get(&key).and_then(Weak::upgrade) {
        return slot;
    }
    let slot = Arc::new(AsyncMutex::new(LeaseModel::default()));
    slots.insert(key, Arc::downgrade(&slot));
    slot
}

pub struct ProfileLease {
    id: u64,
    slot: Option<Arc<Slot>>,
    pub(crate) browser: Arc<SharedBrowser>,
    pub(crate) target_id: Option<String>,
    pub(crate) session_id: Option<String>,
}

pub async fn delete_named_profile(layout: &super::ProfileLayout, name: &str) -> Result<(), String> {
    let name = super::validate_named_profile(name)?;
    let dir = layout.dir_for(&ProfileKind::Named { name: name.into() });
    let slot = slot_for(&dir);
    let model = slot.lock().await;
    if model.handle().is_some() {
        return Err(format!(
            "profile {name} is in use; switch or close every browser using it first"
        ));
    }
    let metadata = tokio::fs::symlink_metadata(&dir)
        .await
        .map_err(|_| format!("profile {name} does not exist"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!("profile {name} must be a real profile directory"));
    }
    tokio::fs::remove_dir_all(&dir)
        .await
        .map_err(|error| format!("could not delete profile {name}: {error}"))
}

impl ProfileLease {
    pub(crate) async fn acquire_with<F, Fut>(
        dir: PathBuf,
        exclusive: bool,
        launch: F,
    ) -> Result<Self, String>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<SharedBrowser, String>> + Send + 'static,
    {
        let slot = slot_for(&dir);
        let (reply, result) = oneshot::channel();
        // The task owns launch and cleanup even if its surface cancels the request.
        tokio::spawn(async move {
            let acquired = async {
                let mut model = if exclusive {
                    slot.try_lock().map_err(|_| "browser profile is in use")?
                } else {
                    slot.lock().await
                };
                if model.handle().is_some_and(|browser| !browser.is_alive()) {
                    if let Some(browser) = model.retire() {
                        browser.stop(CLOSE_DEADLINE, model.generation()).await;
                    }
                }
                if !model.can_acquire(exclusive) {
                    return Err("browser profile is in use".into());
                }
                if model.handle().is_none() {
                    model.begin_launch();
                    model.install(Arc::new(launch().await?));
                }
                let (id, browser) = model.acquire(exclusive).ok_or("profile has no browser")?;
                Ok(Self {
                    id,
                    slot: Some(slot.clone()),
                    browser,
                    target_id: None,
                    session_id: None,
                })
            }
            .await;
            let _ = reply.send(acquired);
        });
        result
            .await
            .map_err(|_| "profile launch task stopped".to_string())?
    }

    pub fn leased_dirs() -> HashSet<PathBuf> {
        slots()
            .lock()
            .iter()
            .filter(|(_, slot)| slot.strong_count() > 0)
            .map(|(dir, _)| dir.clone())
            .collect()
    }

    pub(crate) async fn release(mut self, deadline: Duration) -> Option<CloseOutcome> {
        let task = self.release_task(deadline)?;
        task.await.ok().flatten()
    }

    fn release_task(
        &mut self,
        deadline: Duration,
    ) -> Option<tokio::task::JoinHandle<Option<CloseOutcome>>> {
        self.slot.as_ref()?;
        let executor = match tokio::runtime::Handle::try_current() {
            Ok(executor) => executor,
            Err(_) => {
                self.browser.client.close("host runtime stopped");
                if let Some(process) = &self.browser.process {
                    process.kill();
                }
                self.slot.take();
                return None;
            }
        };
        let slot = self.slot.take()?;
        let id = self.id;
        let browser = self.browser.clone();
        let target_id = self.target_id.take();
        let session_id = self.session_id.take();
        Some(executor.spawn(async move {
            let mut model = slot.lock().await;
            let same_browser = model
                .handle()
                .is_some_and(|current| Arc::ptr_eq(&current, &browser));
            if same_browser && !browser.is_alive() {
                if let Some(retired) = model.retire() {
                    return retired.stop(deadline, model.generation()).await;
                }
            }
            if same_browser {
                if let Some(target_id) = target_id {
                    close_target_accepting_beforeunload(
                        &browser.client,
                        &target_id,
                        session_id.as_deref(),
                        deadline,
                    )
                    .await;
                }
            }
            match model.release(id) {
                Some(last) => last.stop(deadline, model.generation()).await,
                None => None,
            }
        }))
    }
}

struct CloseAuthGuard<'a> {
    client: &'a CdpClient,
    session_id: Option<&'a str>,
    token: u64,
    authorized: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for CloseAuthGuard<'_> {
    fn drop(&mut self) {
        self.authorized
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if let Some(sid) = self.session_id {
            self.client.disarm_dialog_owner(sid, Some(self.token));
        }
    }
}

/// Closes a page target while accepting only a `beforeunload` dialog causally raised by closing it.
/// Dialog generations and causal ownership are stamped by the transport's ordered reader/writer.
async fn close_target_accepting_beforeunload(
    client: &CdpClient,
    target_id: &str,
    session_id: Option<&str>,
    deadline: Duration,
) {
    let close_token = client.allocate_dialog_token();
    let authorized = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let _auth_guard = CloseAuthGuard {
        client,
        session_id,
        token: close_token,
        authorized: authorized.clone(),
    };
    let mut dialogs = session_id.map(|sid| client.subscribe_wire_dialogs(sid));
    let close_gate_flag = authorized.clone();
    let close_gate: crate::DispatchGate =
        Arc::new(move || close_gate_flag.load(std::sync::atomic::Ordering::SeqCst));
    let close =
        client.send_causal_close_target(target_id, session_id, close_token, deadline, close_gate);
    tokio::pin!(close);
    let mut handled_gen: Option<u64> = None;
    let mut pending_accept: Option<(
        u64,
        std::pin::Pin<
            Box<dyn Future<Output = Result<serde_json::Value, crate::CdpError>> + Send + '_>,
        >,
    )> = None;
    let result = loop {
        tokio::select! {
            result = &mut close => break result,
            accept_result = async {
                match pending_accept.as_mut() {
                    Some((_, fut)) => fut.await,
                    None => std::future::pending().await,
                }
            } => {
                pending_accept = None;
                if let Err(error) = accept_result {
                    log::debug!("[browser] close beforeunload reply {target_id}: {error}");
                }
            }
            event = async {
                match dialogs.as_mut() {
                    Some(stream) => stream.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                let Some(first) = event else {
                    dialogs = None;
                    continue;
                };
                let mut latest = match first {
                    crate::WireDialogEvent::Opening(opening) => Some(opening),
                    crate::WireDialogEvent::Closed => None,
                };
                if let Some(stream) = dialogs.as_mut() {
                    while let Ok(queued) = stream.try_recv() {
                        latest = match queued {
                            crate::WireDialogEvent::Opening(opening) => Some(opening),
                            crate::WireDialogEvent::Closed => None,
                        };
                    }
                }
                if let Some((active_gen, _)) = &pending_accept {
                    let still_matching = latest.as_ref().is_some_and(|opening| {
                        opening.generation == *active_gen
                            && opening.owner_token == Some(close_token)
                            && opening.dialog_type.eq_ignore_ascii_case("beforeunload")
                    });
                    if !still_matching {
                        pending_accept = None;
                    }
                }
                if let Some(opening) = latest {
                    if opening.owner_token == Some(close_token)
                        && opening.dialog_type.eq_ignore_ascii_case("beforeunload")
                        && handled_gen != Some(opening.generation)
                    {
                        handled_gen = Some(opening.generation);
                        if let Some(sid) = session_id {
                            let flag = authorized.clone();
                            let gate: crate::DispatchGate = Arc::new(move || {
                                flag.load(std::sync::atomic::Ordering::SeqCst)
                            });
                            pending_accept = Some((
                                opening.generation,
                                Box::pin(client.send_dialog_accept(
                                    sid,
                                    opening.generation,
                                    close_token,
                                    deadline,
                                    gate,
                                )),
                            ));
                        }
                    }
                }
            }
        }
    };
    if let Err(error) = result {
        log::debug!("[browser] close profile target {target_id}: {error}");
    }
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        self.release_task(CLOSE_DEADLINE);
    }
}
