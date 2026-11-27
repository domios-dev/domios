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
    pub startup_targets: Mutex<Vec<String>>,
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
        self.stop_with(deadline, generation, false).await
    }

    async fn stop_with(
        &self,
        deadline: Duration,
        generation: u64,
        force_kill: bool,
    ) -> Option<CloseOutcome> {
        let outcome = if force_kill {
            match &self.process {
                Some(process) => Some(process.force_close().await),
                None => None,
            }
        } else {
            let close = async {
                let _ = self.client.send("Browser.close", json!({})).await;
            };
            match &self.process {
                Some(process) => Some(process.close_with(close, deadline).await),
                None => {
                    let _ = tokio::time::timeout(deadline, close).await;
                    None
                }
            }
        };
        self.client.close(if force_kill {
            "shared browser discarded"
        } else {
            "shared browser stopped"
        });
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

    pub(crate) async fn bind_target(&mut self, target_id: String, session_id: String) {
        self.target_id = Some(target_id.clone());
        self.session_id = Some(session_id);
        // Close Chrome's initial unowned startup tab once an owned page target exists.
        let stale = std::mem::take(&mut *self.browser.startup_targets.lock());
        for stale_id in stale {
            if stale_id == target_id {
                continue;
            }
            if let Err(error) = self
                .browser
                .client
                .send_to(
                    None,
                    "Target.closeTarget",
                    json!({ "targetId": stale_id }),
                    CLOSE_DEADLINE,
                )
                .await
            {
                log::debug!("[browser] close startup target {stale_id}: {error}");
            }
        }
    }

    pub(crate) async fn release(mut self, deadline: Duration) -> Option<CloseOutcome> {
        let task = self.release_task(deadline, TargetClosePolicy::AllowBeforeUnloadAccept)?;
        task.await.ok().flatten()
    }

    pub(crate) async fn release_without_dialog_answer(
        mut self,
        deadline: Duration,
    ) -> Option<CloseOutcome> {
        let task = self.release_task(deadline, TargetClosePolicy::NeverAnswerDialog)?;
        task.await.ok().flatten()
    }

    fn release_task(
        &mut self,
        deadline: Duration,
        policy: TargetClosePolicy,
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
            let mut force_process_teardown = false;
            if same_browser {
                if let Some(target_id) = target_id {
                    match policy {
                        TargetClosePolicy::AllowBeforeUnloadAccept => {
                            close_owned_target(
                                &browser.client,
                                &target_id,
                                session_id.as_deref(),
                                deadline,
                            )
                            .await;
                        }
                        TargetClosePolicy::NeverAnswerDialog => {
                            force_process_teardown = discard_owned_target(
                                &browser.client,
                                &target_id,
                                session_id.as_deref(),
                                model.has_other_leases(id),
                                deadline,
                            )
                            .await;
                        }
                    }
                }
            }
            match model.release(id) {
                Some(last) => {
                    last.stop_with(deadline, model.generation(), force_process_teardown)
                        .await
                }
                None => None,
            }
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetClosePolicy {
    AllowBeforeUnloadAccept,
    NeverAnswerDialog,
}

async fn target_still_open(client: &CdpClient, target_id: &str, deadline: Duration) -> bool {
    if client.close_reason().is_some() {
        return false;
    }
    let Ok(result) = client
        .send_to(None, "Target.getTargets", json!({}), deadline)
        .await
    else {
        return false;
    };
    crate::cdp_wire::parse_targets(&result)
        .iter()
        .any(|target| target.target_id == target_id)
}

async fn close_owned_target(
    client: &CdpClient,
    target_id: &str,
    session_id: Option<&str>,
    deadline: Duration,
) {
    if let Err(error) = client
        .send_to(
            None,
            "Target.closeTarget",
            json!({ "targetId": target_id }),
            deadline,
        )
        .await
    {
        log::debug!("[browser] close profile target {target_id}: {error}");
    }
    let Some(session_id) = session_id else {
        return;
    };
    if !target_still_open(client, target_id, deadline).await {
        return;
    }
    if let Err(error) = client
        .send_to(
            Some(session_id),
            "Page.handleJavaScriptDialog",
            json!({ "accept": true }),
            deadline,
        )
        .await
    {
        log::debug!("[browser] dismiss dialog on closing target {target_id}: {error}");
    }
    if let Err(error) = client
        .send_to(
            None,
            "Target.closeTarget",
            json!({ "targetId": target_id }),
            deadline,
        )
        .await
    {
        log::debug!("[browser] retry close profile target {target_id}: {error}");
    }
}

async fn discard_owned_target(
    client: &CdpClient,
    target_id: &str,
    session_id: Option<&str>,
    has_siblings: bool,
    deadline: Duration,
) -> bool {
    if let Err(error) = client
        .send_to(
            None,
            "Target.closeTarget",
            json!({ "targetId": target_id }),
            deadline,
        )
        .await
    {
        log::debug!("[browser] discard target {target_id}: {error}");
    }
    if !target_still_open(client, target_id, deadline).await {
        return false;
    }
    if !has_siblings {
        return true;
    }
    // Shared profile: tear down only this page's renderer, never answering its dialog.
    if let Some(session_id) = session_id {
        if let Err(error) = client
            .send_to(Some(session_id), "Page.crash", json!({}), deadline)
            .await
        {
            log::debug!("[browser] crash discarded target {target_id}: {error}");
        }
        if let Err(error) = client
            .send_to(
                None,
                "Target.closeTarget",
                json!({ "targetId": target_id }),
                deadline,
            )
            .await
        {
            log::debug!("[browser] retry discard target {target_id}: {error}");
        }
    }
    false
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        self.release_task(CLOSE_DEADLINE, TargetClosePolicy::AllowBeforeUnloadAccept);
    }
}
