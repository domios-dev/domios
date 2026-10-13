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

    async fn stop(&self, deadline: Duration) -> Option<CloseOutcome> {
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
        if !self.profile.is_persistent() {
            let _ = tokio::fs::remove_dir_all(&self.dir).await;
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
}

impl ProfileLease {
    pub(crate) async fn acquire_with<F, Fut>(dir: PathBuf, launch: F) -> Result<Self, String>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<SharedBrowser, String>> + Send + 'static,
    {
        let slot = slot_for(&dir);
        let (reply, result) = oneshot::channel();
        // The task owns launch and cleanup even if its surface cancels the request.
        tokio::spawn(async move {
            let acquired = async {
                let mut model = slot.lock().await;
                if model.handle().is_some_and(|browser| !browser.is_alive()) {
                    if let Some(browser) = model.retire() {
                        browser.stop(CLOSE_DEADLINE).await;
                    }
                }
                if model.handle().is_none() {
                    model.install(Arc::new(launch().await?));
                }
                let (id, browser) = model.acquire().ok_or("profile has no browser")?;
                Ok(Self {
                    id,
                    slot: Some(slot.clone()),
                    browser,
                    target_id: None,
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
        Some(executor.spawn(async move {
            let mut model = slot.lock().await;
            let same_browser = model.handle().is_some_and(|current| Arc::ptr_eq(&current, &browser));
            if same_browser && !browser.is_alive() {
                if let Some(retired) = model.retire() { return retired.stop(deadline).await; }
            }
            if same_browser {
                if let Some(target_id) = target_id {
                    let closed = browser.client.send_to(None, "Target.closeTarget", json!({ "targetId": target_id }), deadline).await;
                    if !matches!(closed, Ok(ref result) if result.get("success").and_then(serde_json::Value::as_bool) == Some(true)) {
                        let retired = model.retire();
                        if let Some(retired) = retired { return retired.stop(deadline).await; }
                    }
                }
            }
            match model.release(id) {
                Some(last) => last.stop(deadline).await,
                None => None,
            }
        }))
    }
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        self.release_task(CLOSE_DEADLINE);
    }
}
