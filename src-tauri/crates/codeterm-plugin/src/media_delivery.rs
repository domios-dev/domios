//! Opaque, frame-scoped access to passive files owned by a plugin.
use crate::view_host::{ViewCallContext, ViewProvider, ViewRegistry, ViewRegistryHandle};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Weak},
    time::{Duration, Instant},
};

mod format;
pub use format::{MediaDeliveryDescriptor, MediaFile, MediaType};

pub const MAX_MEDIA_BYTES: u64 = 256 * 1024 * 1024;
const FRAME_TTL: Duration = Duration::from_secs(1800);
const HANDLE_TTL: Duration = Duration::from_secs(600);
const MAX_FRAMES: usize = 128;
const MAX_HANDLES: usize = 16;
static STORE: LazyLock<Mutex<MediaStore>> = LazyLock::new(|| Mutex::new(MediaStore::default()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRefusal {
    Unknown,
    Expired,
    Foreign,
}

pub struct ResolvedMedia {
    pub path: PathBuf,
    pub file_name: String,
    pub content_type: String,
}
pub struct OpenedMedia {
    pub file: File,
    pub path: PathBuf,
    pub file_name: String,
    pub content_type: String,
}

#[derive(Serialize)]
pub struct MediaHandle {
    pub handle: String,
    pub url: Option<String>,
    pub content_type: String,
    pub file_name: String,
    pub expires_in_seconds: u64,
}

struct Frame {
    plugin: String,
    view_lease: String,
    context: ViewCallContext,
    provider: Weak<dyn ViewProvider>,
    provider_registration: String,
    registry: Weak<ViewRegistry>,
    expires: Instant,
    handles: HashMap<String, Entry>,
}
struct Entry {
    root: PathBuf,
    relative: String,
    resolved: ResolvedMedia,
    file: Arc<Mutex<File>>,
    size: u64,
    kind: MediaType,
    expires: Instant,
}
#[derive(Default)]
struct MediaStore {
    frames: HashMap<String, Frame>,
}

pub fn valid_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn valid_plugin_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}
fn token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

impl MediaStore {
    fn cleanup(&mut self, now: Instant) {
        self.frames.retain(|_, frame| {
            frame.handles.retain(|_, entry| entry.expires > now);
            frame.expires > now && frame.provider.strong_count() > 0
        });
    }
    fn frame(&self, plugin: &str, frame: &str, now: Instant) -> Result<&Frame, MediaRefusal> {
        let value = self.frames.get(frame).ok_or(MediaRefusal::Unknown)?;
        if value.plugin != plugin {
            return Err(MediaRefusal::Foreign);
        }
        let provider = value.provider.upgrade().ok_or(MediaRefusal::Expired)?;
        let registry = value.registry.upgrade().ok_or(MediaRefusal::Expired)?;
        let current = registry.get(plugin).ok_or(MediaRefusal::Expired)?;
        if value.expires <= now
            || !Arc::ptr_eq(&provider, &current)
            || registry.registration_id(plugin).as_deref()
                != Some(value.provider_registration.as_str())
        {
            return Err(MediaRefusal::Expired);
        }
        Ok(value)
    }
    fn entry(
        &self,
        plugin: &str,
        frame: &str,
        handle: &str,
        now: Instant,
    ) -> Result<&Entry, MediaRefusal> {
        let value = self
            .frame(plugin, frame, now)?
            .handles
            .get(handle)
            .ok_or(MediaRefusal::Unknown)?;
        if value.expires <= now {
            return Err(MediaRefusal::Expired);
        }
        Ok(value)
    }
}

/// Called only by the authenticated host document loader.
pub struct FrameRegistration {
    pub document: String,
    pub context: ViewCallContext,
    pub media_frame: String,
    pub view_lease: String,
}

pub fn create_frame(
    provider: &Arc<dyn ViewProvider>,
    registry: &ViewRegistryHandle,
    context: ViewCallContext,
) -> Result<FrameRegistration, MediaRefusal> {
    create_frame_at_key(provider, registry, context, token())
}

/// A proven original document replaces its retired registration, never an occupied one.
pub fn reregister_frame(
    provider: &Arc<dyn ViewProvider>,
    registry: &ViewRegistryHandle,
    context: ViewCallContext,
    presentation: &str,
) -> Result<FrameRegistration, MediaRefusal> {
    if !valid_token(presentation) {
        return Err(MediaRefusal::Unknown);
    }
    create_frame_at_key(provider, registry, context, presentation.into())
}

fn create_frame_at_key(
    provider: &Arc<dyn ViewProvider>,
    registry: &ViewRegistryHandle,
    mut context: ViewCallContext,
    frame: String,
) -> Result<FrameRegistration, MediaRefusal> {
    let provider_registration = registry
        .registration_id(provider.id())
        .ok_or(MediaRefusal::Unknown)?;
    if !registry
        .get(provider.id())
        .is_some_and(|current| Arc::ptr_eq(&current, provider))
    {
        return Err(MediaRefusal::Unknown);
    }
    if !valid_plugin_id(provider.id())
        || context.plugin != provider.id()
        || context.origin != crate::view_host::ViewCallOrigin::Document
    {
        return Err(MediaRefusal::Unknown);
    }
    let mut store = STORE.lock();
    store.cleanup(Instant::now());
    if store.frames.contains_key(&frame)
        || store.frames.len() >= MAX_FRAMES
        || store
            .frames
            .values()
            .filter(|f| f.plugin == provider.id())
            .count()
            >= 16
    {
        return Err(MediaRefusal::Unknown);
    }
    let view_lease = token();
    let document = uuid::Uuid::new_v4().simple().to_string();
    context.frame = Some(document.clone());
    store.frames.insert(
        frame.clone(),
        Frame {
            plugin: provider.id().into(),
            view_lease: view_lease.clone(),
            context: context.clone(),
            provider: Arc::downgrade(provider),
            provider_registration,
            registry: Arc::downgrade(registry),
            expires: Instant::now() + FRAME_TTL,
            handles: HashMap::new(),
        },
    );
    Ok(FrameRegistration {
        document,
        context,
        media_frame: frame,
        view_lease,
    })
}

/// The presentation frame key is not a backend view-call credential.
pub fn view_frame(
    plugin: &str,
    lease: &str,
) -> Result<(Arc<dyn ViewProvider>, ViewCallContext), MediaRefusal> {
    if !valid_token(lease) {
        return Err(MediaRefusal::Unknown);
    }
    let mut store = STORE.lock();
    let now = Instant::now();
    store.cleanup(now);
    let key = store
        .frames
        .iter()
        .find(|(_, frame)| frame.plugin == plugin && frame.view_lease == lease)
        .map(|(key, _)| key.clone())
        .ok_or(MediaRefusal::Unknown)?;
    let frame = store.frame(plugin, &key, now)?;
    Ok((
        frame.provider.upgrade().ok_or(MediaRefusal::Expired)?,
        frame.context.clone(),
    ))
}

/// Only authenticated host media routes use the presentation key to resolve this context.
pub fn frame_context(plugin: &str, key: &str) -> Result<ViewCallContext, MediaRefusal> {
    let mut store = STORE.lock();
    store.cleanup(Instant::now());
    Ok(store.frame(plugin, key, Instant::now())?.context.clone())
}

/// Host admission checks the captured invocation without accepting an echoed scope or lease.
pub fn context_provider(context: &ViewCallContext) -> Result<Arc<dyn ViewProvider>, MediaRefusal> {
    let mut store = STORE.lock();
    let now = Instant::now();
    store.cleanup(now);
    let key = store
        .frames
        .iter()
        .find(|(_, frame)| frame.context == *context)
        .map(|(key, _)| key.clone())
        .ok_or(MediaRefusal::Unknown)?;
    store
        .frame(&context.plugin, &key, now)?
        .provider
        .upgrade()
        .ok_or(MediaRefusal::Expired)
}

pub fn context_is_live(context: &ViewCallContext) -> Result<(), MediaRefusal> {
    context_provider(context).map(|_| ())
}

pub fn active_view_contexts(plugin: &str) -> Vec<ViewCallContext> {
    let mut store = STORE.lock();
    let now = Instant::now();
    store.cleanup(now);
    store
        .frames
        .keys()
        .filter_map(|key| {
            store
                .frame(plugin, key, now)
                .ok()
                .map(|frame| frame.context.clone())
        })
        .collect()
}

pub fn revoke_view_frame(plugin: &str, lease: &str) {
    STORE
        .lock()
        .frames
        .retain(|_, frame| frame.plugin != plugin || frame.view_lease != lease);
}

pub fn revoke_frame(plugin: &str, frame: &str) {
    let mut store = STORE.lock();
    if store
        .frames
        .get(frame)
        .is_some_and(|value| value.plugin == plugin)
    {
        store.frames.remove(frame);
    }
}
pub fn revoke_plugin(plugin: &str) {
    STORE
        .lock()
        .frames
        .retain(|_, frame| frame.plugin != plugin);
}

pub fn frame_provider(plugin: &str, frame: &str) -> Result<Arc<dyn ViewProvider>, MediaRefusal> {
    let mut store = STORE.lock();
    store.cleanup(Instant::now());
    store
        .frame(plugin, frame, Instant::now())?
        .provider
        .upgrade()
        .ok_or(MediaRefusal::Expired)
}

/// Backend output is resolved against the host-chosen plugin data directory only.
pub fn register_media(
    plugin: &str,
    frame: &str,
    source: MediaFile,
) -> Result<MediaHandle, MediaRefusal> {
    register_at_root(
        &mut STORE.lock(),
        plugin,
        frame,
        &codeterm_paths::data_dir().join(plugin),
        source,
        Instant::now(),
    )
}
fn register_at_root(
    store: &mut MediaStore,
    plugin: &str,
    frame: &str,
    root: &Path,
    source: MediaFile,
    now: Instant,
) -> Result<MediaHandle, MediaRefusal> {
    let owner = store.frame(plugin, frame, now)?;
    let provider = owner.provider.upgrade().ok_or(MediaRefusal::Expired)?;
    let descriptor = provider.media_delivery().ok_or(MediaRefusal::Unknown)?;
    if !descriptor.valid()
        || !descriptor.types.contains(&source.content_type)
        || owner.handles.len() >= MAX_HANDLES
    {
        return Err(MediaRefusal::Unknown);
    }
    let opened = open_owned_at_root(root, &source, &descriptor)?;
    let size = opened
        .file
        .metadata()
        .map_err(|_| MediaRefusal::Unknown)?
        .len();
    let file_name = opened.file_name;
    let content_type = opened.content_type;
    let file = opened.file;
    let path = opened.path;
    let handle = token();
    let expires = (now + HANDLE_TTL).min(owner.expires);
    let url = source
        .content_type
        .playback()
        .then(|| format!("/api/plugins/{plugin}/media/{frame}/{handle}"));
    store
        .frames
        .get_mut(frame)
        .ok_or(MediaRefusal::Expired)?
        .handles
        .insert(
            handle.clone(),
            Entry {
                root: root.to_path_buf(),
                relative: source.path,
                resolved: ResolvedMedia {
                    path,
                    file_name: file_name.clone(),
                    content_type: content_type.clone(),
                },
                file: Arc::new(Mutex::new(file)),
                size,
                kind: source.content_type,
                expires,
            },
        );
    Ok(MediaHandle {
        handle,
        url,
        content_type,
        file_name,
        expires_in_seconds: expires.saturating_duration_since(now).as_secs(),
    })
}
fn confined_path(root: &Path, relative: &str) -> Result<PathBuf, MediaRefusal> {
    let expected = root
        .parent()
        .ok_or(MediaRefusal::Unknown)?
        .canonicalize()
        .map_err(|_| MediaRefusal::Unknown)?
        .join(root.file_name().ok_or(MediaRefusal::Unknown)?);
    let root = root.canonicalize().map_err(|_| MediaRefusal::Unknown)?;
    if root != expected {
        return Err(MediaRefusal::Unknown);
    }
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|_| MediaRefusal::Unknown)?;
    if !path.starts_with(&root) {
        return Err(MediaRefusal::Unknown);
    }
    Ok(path)
}

/// Shared validation seam for a host-stamped producer plugin, without frame/run authority.
pub fn open_owned_media(
    plugin: &str,
    source: &MediaFile,
    descriptor: &MediaDeliveryDescriptor,
) -> Result<OpenedMedia, MediaRefusal> {
    if !valid_plugin_id(plugin) {
        return Err(MediaRefusal::Unknown);
    }
    open_owned_at_root(&codeterm_paths::data_dir().join(plugin), source, descriptor)
}
fn open_owned_at_root(
    root: &Path,
    source: &MediaFile,
    descriptor: &MediaDeliveryDescriptor,
) -> Result<OpenedMedia, MediaRefusal> {
    if !descriptor.valid()
        || !descriptor.types.contains(&source.content_type)
        || source.path.len() > 4096
        || source.path.contains(['\\', ':', '\0'])
    {
        return Err(MediaRefusal::Unknown);
    }
    let mut file = codeterm_blueprint_types::code_source::open_file(root, &source.path)
        .map_err(|_| MediaRefusal::Unknown)?;
    let size = file.metadata().map_err(|_| MediaRefusal::Unknown)?.len();
    if size == 0 || size > MAX_MEDIA_BYTES {
        return Err(MediaRefusal::Unknown);
    }
    let mut head = [0; 32];
    let count = file.read(&mut head).map_err(|_| MediaRefusal::Unknown)?;
    if !source.content_type.matches(&head[..count]) {
        return Err(MediaRefusal::Unknown);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| MediaRefusal::Unknown)?;
    Ok(OpenedMedia {
        file,
        path: confined_path(root, &source.path)?,
        file_name: format::safe_name(
            source.file_name.as_deref().unwrap_or("media"),
            source.content_type,
        ),
        content_type: source.content_type.as_str().into(),
    })
}
fn open_entry(entry: &Entry) -> Result<OpenedMedia, MediaRefusal> {
    open_owned_at_root(
        &entry.root,
        &MediaFile {
            path: entry.relative.clone(),
            content_type: entry.kind,
            file_name: Some(entry.resolved.file_name.clone()),
        },
        &MediaDeliveryDescriptor {
            types: vec![entry.kind],
        },
    )
}
pub fn frame_owns_handle(plugin: &str, frame: &str, handle: &str) -> Result<(), MediaRefusal> {
    let store = STORE.lock();
    let owner = store.frame(plugin, frame, Instant::now())?;
    if !owner.handles.contains_key(handle) {
        return Err(MediaRefusal::Foreign);
    }
    store
        .entry(plugin, frame, handle, Instant::now())
        .map(|_| ())
}
/// Atomically check exact-frame ownership and open a freshly confined file for gesture actions.
pub fn open_frame_media(
    plugin: &str,
    frame: &str,
    handle: &str,
) -> Result<OpenedMedia, MediaRefusal> {
    let store = STORE.lock();
    let owner = store.frame(plugin, frame, Instant::now())?;
    if !owner.handles.contains_key(handle) {
        return Err(MediaRefusal::Foreign);
    }
    open_entry(store.entry(plugin, frame, handle, Instant::now())?)
}
/// Non-frame consumers must establish their own host authority before calling this resolver.
pub fn open_plugin_media(plugin: &str, handle: &str) -> Result<OpenedMedia, MediaRefusal> {
    let store = STORE.lock();
    for (frame_id, frame) in &store.frames {
        if frame.handles.contains_key(handle) {
            return open_entry(store.entry(plugin, frame_id, handle, Instant::now())?);
        }
    }
    Err(MediaRefusal::Unknown)
}
pub fn resolve_plugin_media(plugin: &str, handle: &str) -> Result<ResolvedMedia, MediaRefusal> {
    let opened = open_plugin_media(plugin, handle)?;
    Ok(ResolvedMedia {
        path: opened.path,
        file_name: opened.file_name,
        content_type: opened.content_type,
    })
}

#[derive(Clone)]
pub struct Playback {
    plugin: String,
    frame: String,
    handle: String,
    pub size: u64,
    pub content_type: String,
    file: Arc<Mutex<File>>,
}
pub fn playback(plugin: &str, frame: &str, handle: &str) -> Result<Playback, MediaRefusal> {
    let store = STORE.lock();
    let entry = store.entry(plugin, frame, handle, Instant::now())?;
    if !entry.resolved.content_type.starts_with("audio/")
        && !entry.resolved.content_type.starts_with("video/")
    {
        return Err(MediaRefusal::Unknown);
    }
    Ok(Playback {
        plugin: plugin.into(),
        frame: frame.into(),
        handle: handle.into(),
        size: entry.size,
        content_type: entry.resolved.content_type.clone(),
        file: entry.file.clone(),
    })
}
impl Playback {
    /// The caller offloads each bounded read; clones share one explicitly positioned cursor.
    pub fn read(&self, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        STORE
            .lock()
            .entry(&self.plugin, &self.frame, &self.handle, Instant::now())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
        if length > 65536 || offset > self.size {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        let mut file = self.file.lock();
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; length.min(self.size.saturating_sub(offset) as usize)];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests;
