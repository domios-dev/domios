use super::{model::*, upload::SelectedFile};
use parking_lot::Mutex;
use reqwest::Url;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, LazyLock, Weak,
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;

pub trait TransferAuthority: Send + Sync {
    fn authorize(&self, url: &Url) -> Result<()>;
    fn credentials(&self, credential: &Credential) -> Result<BTreeMap<String, String>>;
    fn readable(&self, path: &Path) -> bool;
}
pub struct TransferSession {
    pub(crate) plugin: String,
    pub(crate) epoch: String,
    pub(crate) root: PathBuf,
    pub(crate) limits: BinaryTransferDescriptor,
    pub(crate) authority: Arc<dyn TransferAuthority>,
    revoked: TransferAbort,
    pub(crate) active: AtomicUsize,
    selected: Mutex<HashMap<String, Selection>>,
    #[cfg(test)]
    pub(crate) test_network: Mutex<Option<Arc<dyn super::network::TransferNetwork>>>,
}
struct Selection {
    origin: String,
    expires: Instant,
    input: SelectedFile,
}
static INPUT_PREPARATIONS: AtomicUsize = AtomicUsize::new(0);
struct PreparationPermit<'a>(&'a AtomicUsize);
impl Drop for PreparationPermit<'_> {
    fn drop(&mut self) {
        INPUT_PREPARATIONS.fetch_sub(1, Ordering::AcqRel);
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone)]
pub struct TransferAbort(watch::Sender<bool>);
impl Default for TransferAbort {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}
impl TransferAbort {
    pub fn abort(&self) {
        self.0.send_replace(true);
    }
    pub fn is_aborted(&self) -> bool {
        *self.0.borrow()
    }
    pub(crate) async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        loop {
            if *receiver.borrow_and_update() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}
impl TransferSession {
    pub fn new(
        plugin: &str,
        limits: BinaryTransferDescriptor,
        authority: Arc<dyn TransferAuthority>,
    ) -> Result<Arc<Self>> {
        Self::at_root(
            plugin,
            codeterm_paths::data_dir().join(plugin),
            limits,
            authority,
        )
    }
    pub(crate) fn at_root(
        plugin: &str,
        root: PathBuf,
        limits: BinaryTransferDescriptor,
        authority: Arc<dyn TransferAuthority>,
    ) -> Result<Arc<Self>> {
        if !crate::media_delivery::valid_plugin_id(plugin) || !limits.valid() {
            return Err(TransferRefusal::InvalidRequest);
        }
        Ok(Arc::new(Self {
            plugin: plugin.into(),
            epoch: uuid::Uuid::new_v4().simple().to_string(),
            root,
            limits,
            authority,
            revoked: TransferAbort::default(),
            #[cfg(test)]
            test_network: Mutex::new(None),
            active: AtomicUsize::new(0),
            selected: Mutex::new(HashMap::new()),
        }))
    }
    pub(crate) fn network(&self) -> Arc<dyn super::network::TransferNetwork> {
        #[cfg(test)]
        if let Some(network) = self.test_network.lock().as_ref() {
            return network.clone();
        }
        Arc::new(super::network::HttpsNetwork)
    }
    pub fn epoch(&self) -> &str {
        &self.epoch
    }
    pub fn plugin_id(&self) -> &str {
        &self.plugin
    }
    pub fn revoke(&self) {
        self.revoked.abort();
        self.selected.lock().clear();
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.revoked.is_aborted() {
            Err(TransferRefusal::NotAllowed)
        } else {
            Ok(())
        }
    }
    pub(crate) async fn cancelled(&self) {
        self.revoked.cancelled().await;
    }
    pub fn deadline(&self, request: &TransferRequest) -> Result<Instant> {
        let millis = request.timeout_ms.unwrap_or(self.limits.timeout_ms);
        if millis == 0 || millis > self.limits.timeout_ms {
            return Err(TransferRefusal::InvalidRequest);
        }
        let deadline = Instant::now() + Duration::from_millis(millis);
        Ok(codeterm_decision::physical::deadline().map_or(deadline, |bound| bound.min(deadline)))
    }
    pub(crate) fn authorize(&self, url: &Url) -> Result<()> {
        self.check()?;
        super::network::validate_url(url)?;
        self.authority.authorize(url)
    }
    /// Host-only: the caller must already possess an explicitly selected, confined file handle.
    pub fn select_input(
        &self,
        origin: &str,
        file: File,
        name: &str,
        mime: &str,
        expected: Option<&FileIdentity>,
    ) -> Result<TransferInputGrant> {
        let requested = Instant::now() + Duration::from_millis(self.limits.timeout_ms);
        let control = InputPreparation {
            abort: TransferAbort::default(),
            deadline: codeterm_decision::physical::deadline()
                .map_or(requested, |bound| bound.min(requested)),
        };
        self.select_input_bounded(origin, file, name, mime, expected, &control)
    }
    pub fn select_input_bounded(
        &self,
        origin: &str,
        file: File,
        file_name: &str,
        content_type: &str,
        expected: Option<&FileIdentity>,
        control: &InputPreparation,
    ) -> Result<TransferInputGrant> {
        INPUT_PREPARATIONS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 32).then_some(count + 1)
            })
            .map_err(|_| TransferRefusal::Unavailable)?;
        if self
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 8).then_some(count + 1)
            })
            .is_err()
        {
            INPUT_PREPARATIONS.fetch_sub(1, Ordering::AcqRel);
            return Err(TransferRefusal::Unavailable);
        }
        let _permit = PreparationPermit(&self.active);
        let requested = Instant::now() + Duration::from_millis(self.limits.timeout_ms);
        let deadline = control.deadline.min(
            codeterm_decision::physical::deadline().map_or(requested, |bound| bound.min(requested)),
        );
        let url = Url::parse(origin).map_err(|_| TransferRefusal::InvalidRequest)?;
        if url.origin().ascii_serialization() != origin {
            return Err(TransferRefusal::InvalidRequest);
        }
        self.authorize(&url)?;
        let input = SelectedFile::validate(
            file,
            file_name,
            content_type,
            self.limits.max_upload_bytes,
            expected,
            || {
                if control.abort.is_aborted() {
                    return Err(TransferRefusal::Aborted);
                }
                if Instant::now() >= deadline {
                    return Err(TransferRefusal::Deadline);
                }
                self.authorize(&url)
            },
        )?;
        let mut selections = self.selected.lock();
        selections.retain(|_, entry| entry.expires > Instant::now());
        if selections.len() >= 8 {
            return Err(TransferRefusal::Unavailable);
        }
        let handle = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let grant = TransferInputGrant {
            handle: handle.clone(),
            file_name: input.name.clone(),
            content_type: input.mime.clone(),
            size: input.size,
            sha256: input.sha256.clone(),
        };
        selections.insert(
            handle.clone(),
            Selection {
                origin: url.origin().ascii_serialization(),
                expires: Instant::now() + Duration::from_secs(300),
                input,
            },
        );
        Ok(grant)
    }
    pub(crate) fn credential_allowed(&self, url: &Url, credential: &Credential) -> Result<()> {
        if self
            .limits
            .credential_origins
            .get(&credential.binding_key())
            .is_some_and(|origins| origins.contains(&url.origin().ascii_serialization()))
        {
            Ok(())
        } else {
            Err(TransferRefusal::NotAllowed)
        }
    }
    pub fn revoke_input(&self, handle: &str) {
        self.selected.lock().remove(handle);
    }
    pub(crate) fn take_input(&self, handle: &str, origin: &Url) -> Result<SelectedFile> {
        self.check()?;
        let mut selections = self.selected.lock();
        let entry = selections.get(handle).ok_or(TransferRefusal::NotAllowed)?;
        if entry.origin != origin.origin().ascii_serialization() || entry.expires <= Instant::now()
        {
            return Err(TransferRefusal::NotAllowed);
        }
        selections
            .remove(handle)
            .map(|entry| entry.input)
            .ok_or(TransferRefusal::NotAllowed)
    }
}

static SESSIONS: LazyLock<Mutex<HashMap<String, Weak<TransferSession>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub fn activate(session: &Arc<TransferSession>) {
    let previous = {
        let mut sessions = SESSIONS.lock();
        sessions.retain(|_, entry| entry.strong_count() > 0);
        sessions.insert(session.plugin.clone(), Arc::downgrade(session))
    };
    if let Some(previous) = previous
        .and_then(|value| value.upgrade())
        .filter(|previous| previous.epoch != session.epoch)
    {
        previous.revoke();
    }
}
pub fn deactivate(plugin: &str) {
    if let Some(previous) = SESSIONS
        .lock()
        .remove(plugin)
        .and_then(|value| value.upgrade())
    {
        previous.revoke();
    }
}
pub(crate) fn current(session: &TransferSession) -> bool {
    SESSIONS
        .lock()
        .get(&session.plugin)
        .and_then(Weak::upgrade)
        .is_some_and(|value| value.epoch == session.epoch)
}

pub(crate) fn live_session(plugin: &str) -> Option<Arc<TransferSession>> {
    SESSIONS.lock().get(plugin).and_then(Weak::upgrade)
}

pub struct PreparedInputGrant {
    session: Weak<TransferSession>,
    grant: Option<TransferInputGrant>,
}
impl PreparedInputGrant {
    #[cfg(test)]
    pub(crate) fn handle_for_test(&self) -> &str {
        &self.grant.as_ref().unwrap().handle
    }
    pub fn commit(mut self) -> Result<TransferInputGrant> {
        let session = self.session.upgrade().ok_or(TransferRefusal::NotAllowed)?;
        if !current(&session) || session.check().is_err() {
            return Err(TransferRefusal::NotAllowed);
        }
        self.grant.take().ok_or(TransferRefusal::NotAllowed)
    }
}
impl Drop for PreparedInputGrant {
    fn drop(&mut self) {
        if let (Some(session), Some(grant)) = (self.session.upgrade(), self.grant.as_ref()) {
            session.revoke_input(&grant.handle);
        }
    }
}
#[derive(Clone)]
pub struct InputPreparation {
    pub abort: TransferAbort,
    pub deadline: Instant,
}
pub fn prepare_plugin_input(
    plugin: &str,
    destination: &str,
    file: File,
    name: &str,
    mime: &str,
    expected: Option<&FileIdentity>,
) -> Result<PreparedInputGrant> {
    let session = live_session(plugin).ok_or(TransferRefusal::NotAllowed)?;
    let requested = Instant::now() + Duration::from_millis(session.limits.timeout_ms);
    let control = InputPreparation {
        abort: TransferAbort::default(),
        deadline: codeterm_decision::physical::deadline()
            .map_or(requested, |bound| bound.min(requested)),
    };
    prepare_plugin_input_bounded(plugin, destination, file, name, mime, expected, &control)
}
pub fn prepare_plugin_input_bounded(
    plugin: &str,
    destination: &str,
    file: File,
    name: &str,
    mime: &str,
    expected: Option<&FileIdentity>,
    control: &InputPreparation,
) -> Result<PreparedInputGrant> {
    let session = live_session(plugin)
        .filter(|session| current(session))
        .ok_or(TransferRefusal::NotAllowed)?;
    let grant = session.select_input_bounded(destination, file, name, mime, expected, control)?;
    let prepared = PreparedInputGrant {
        session: Arc::downgrade(&session),
        grant: Some(grant),
    };
    if !current(&session) || session.check().is_err() {
        return Err(TransferRefusal::NotAllowed);
    }
    Ok(prepared)
}
pub fn select_plugin_input(
    plugin: &str,
    destination: &str,
    file: File,
    name: &str,
    mime: &str,
    expected: Option<&FileIdentity>,
) -> Result<TransferInputGrant> {
    prepare_plugin_input(plugin, destination, file, name, mime, expected)?.commit()
}

#[cfg(test)]
mod tests;
