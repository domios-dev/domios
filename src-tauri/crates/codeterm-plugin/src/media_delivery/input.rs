use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewInputDescriptor {
    pub accept: Vec<String>,
    pub max_bytes: u64,
    pub max_files: u8,
}
impl ViewInputDescriptor {
    pub fn valid(&self) -> bool {
        !self.accept.is_empty()
            && self.accept.len() <= 10
            && self.max_bytes > 0
            && self.max_bytes <= MAX_MEDIA_BYTES
            && (1..=8).contains(&self.max_files)
            && self.accept.iter().enumerate().all(|(i, mime)| {
                !self.accept[..i].contains(mime)
                    && serde_json::from_value::<MediaType>(serde_json::Value::String(mime.clone()))
                        .is_ok()
            })
    }
}
pub enum IngestSource {
    Opened(File),
    Bytes(Vec<u8>),
    Reader(Box<dyn Read + Send>),
}
pub struct IngestLimits<'a> {
    pub accept: &'a [String],
    pub max_bytes: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestRefusal {
    UnknownFrame,
    TooLarge,
    TypeNotAccepted,
    Unreadable,
    Storage,
    Deadline,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestedInput {
    pub handle: String,
    pub file_name: String,
    pub content_type: String,
    pub size: u64,
    pub sha256: String,
}
pub struct OpenedInput {
    pub media: OpenedMedia,
    pub sha256: String,
}

#[allow(clippy::too_many_arguments)]
pub fn ingest_frame_input(
    plugin: &str,
    frame: &str,
    context: &ViewCallContext,
    source: IngestSource,
    display_name: &str,
    declared_mime: &str,
    limits: IngestLimits<'_>,
    still_current: &dyn Fn() -> bool,
) -> Result<IngestedInput, IngestRefusal> {
    let root = codeterm_paths::data_dir().join(plugin);
    ingest_target(
        IngestTarget {
            plugin,
            frame,
            root: &root,
            context,
            still_current,
        },
        source,
        display_name,
        declared_mime,
        limits,
    )
}
struct IngestTarget<'a> {
    plugin: &'a str,
    frame: &'a str,
    root: &'a Path,
    context: &'a ViewCallContext,
    still_current: &'a dyn Fn() -> bool,
}
#[cfg(test)]
fn ingest_at_root(
    plugin: &str,
    frame: &str,
    root: &Path,
    source: IngestSource,
    display_name: &str,
    hint: &str,
    limits: IngestLimits<'_>,
) -> Result<IngestedInput, IngestRefusal> {
    let context = frame_context(plugin, frame).map_err(|_| IngestRefusal::UnknownFrame)?;
    ingest_target(
        IngestTarget {
            plugin,
            frame,
            root,
            context: &context,
            still_current: &|| true,
        },
        source,
        display_name,
        hint,
        limits,
    )
}
fn ingest_target(
    target: IngestTarget<'_>,
    source: IngestSource,
    display_name: &str,
    hint: &str,
    limits: IngestLimits<'_>,
) -> Result<IngestedInput, IngestRefusal> {
    let IngestTarget {
        plugin,
        frame,
        root,
        context,
        still_current,
    } = target;
    let provider = frame_provider_for_context(plugin, frame, context)
        .map_err(|_| IngestRefusal::UnknownFrame)?;
    let declaration = provider
        .view_inputs()
        .filter(ViewInputDescriptor::valid)
        .ok_or(IngestRefusal::TypeNotAccepted)?;
    if limits.max_bytes == 0
        || limits.max_bytes > declaration.max_bytes
        || limits.accept.is_empty()
        || limits
            .accept
            .iter()
            .any(|mime| !declaration.accept.contains(mime))
    {
        return Err(IngestRefusal::TypeNotAccepted);
    }
    let (mut reader, expected_length): (Box<dyn Read + Send>, Option<u64>) = match source {
        IngestSource::Opened(mut file) => {
            let metadata = file.metadata().map_err(|_| IngestRefusal::Unreadable)?;
            if !metadata.is_file() {
                return Err(IngestRefusal::Unreadable);
            }
            if metadata.len() > limits.max_bytes {
                return Err(IngestRefusal::TooLarge);
            }
            file.seek(SeekFrom::Start(0))
                .map_err(|_| IngestRefusal::Unreadable)?;
            (Box::new(file.take(metadata.len())), Some(metadata.len()))
        }
        IngestSource::Bytes(bytes) => {
            if bytes.len() as u64 > limits.max_bytes {
                return Err(IngestRefusal::TooLarge);
            }
            let size = bytes.len() as u64;
            (Box::new(std::io::Cursor::new(bytes)), Some(size))
        }
        IngestSource::Reader(reader) => (reader, None),
    };
    match std::fs::create_dir(root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(IngestRefusal::Storage),
    }
    let temporary_name = format!("input-{}.bin", uuid::Uuid::new_v4().simple());
    let mut target = codeterm_blueprint_types::owned_output::OwnedOutput::create_child(
        root,
        "inputs",
        &temporary_name,
    )
    .map_err(|_| IngestRefusal::Storage)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut size = 0u64;
    let mut head = Vec::with_capacity(32);
    let mut hash = Sha256::new();
    let mut bytes = vec![0; 65536];
    loop {
        if Instant::now() >= deadline {
            return Err(IngestRefusal::Deadline);
        }
        frame_provider_for_context(plugin, frame, context)
            .map_err(|_| IngestRefusal::UnknownFrame)?;
        let count = reader
            .read(&mut bytes)
            .map_err(|_| IngestRefusal::Unreadable)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or(IngestRefusal::TooLarge)?;
        if size > limits.max_bytes {
            return Err(IngestRefusal::TooLarge);
        }
        let keep = (32 - head.len()).min(count);
        head.extend_from_slice(&bytes[..keep]);
        hash.update(&bytes[..count]);
        target
            .write_all(&bytes[..count])
            .map_err(|_| IngestRefusal::Storage)?;
    }
    if expected_length.is_some_and(|expected| expected != size) {
        return Err(IngestRefusal::Unreadable);
    }
    let types = limits
        .accept
        .iter()
        .filter_map(|mime| {
            serde_json::from_value::<MediaType>(serde_json::Value::String(mime.clone())).ok()
        })
        .filter(|kind| kind.matches(&head))
        .collect::<Vec<_>>();
    let kind = if hint.is_empty() {
        if types.len() != 1 {
            return Err(IngestRefusal::TypeNotAccepted);
        }
        types[0]
    } else {
        *types
            .iter()
            .find(|kind| kind.as_str() == hint)
            .ok_or(IngestRefusal::TypeNotAccepted)?
    };
    if size == 0 {
        return Err(IngestRefusal::TypeNotAccepted);
    }
    let sha256 = format!("{:x}", hash.finalize());
    frame_provider_for_context(plugin, frame, context).map_err(|_| IngestRefusal::UnknownFrame)?;
    let path = target.publish().map_err(|_| IngestRefusal::Storage)?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| IngestRefusal::Storage)?
        .to_string_lossy()
        .replace('\\', "/");
    let current = frame_provider_for_context(plugin, frame, context)
        .map_err(|_| IngestRefusal::UnknownFrame)?
        .view_inputs()
        .filter(ViewInputDescriptor::valid)
        .ok_or(IngestRefusal::TypeNotAccepted)?;
    if size > current.max_bytes || !current.accept.iter().any(|mime| mime == kind.as_str()) {
        return Err(IngestRefusal::TypeNotAccepted);
    }
    let display = display_name.chars().take(100).collect::<String>();
    let source = MediaFile {
        path: relative,
        content_type: kind,
        file_name: Some(display),
    };
    let handle = {
        let mut store = STORE.lock();
        require_frame_context(&store, plugin, frame, context)
            .map_err(|_| IngestRefusal::UnknownFrame)?;
        let handle = register_with_descriptor(
            &mut store,
            plugin,
            frame,
            root,
            source,
            Instant::now(),
            Some(MediaDeliveryDescriptor { types: vec![kind] }),
        )
        .map_err(|_| IngestRefusal::UnknownFrame)?;
        let entry = store
            .frames
            .get_mut(frame)
            .and_then(|frame| frame.handles.get_mut(&handle.handle))
            .ok_or(IngestRefusal::UnknownFrame)?;
        if entry.size != size {
            if let Some(frame) = store.frames.get_mut(frame) {
                frame.handles.remove(&handle.handle);
            }
            return Err(IngestRefusal::Unreadable);
        }
        store
            .frames
            .get_mut(frame)
            .and_then(|frame| frame.handles.get_mut(&handle.handle))
            .ok_or(IngestRefusal::UnknownFrame)?
            .input_sha256 = Some(sha256.clone());
        handle
    };
    let refusal = if Instant::now() >= deadline {
        Some(IngestRefusal::Deadline)
    } else if !still_current() {
        Some(IngestRefusal::UnknownFrame)
    } else {
        frame_provider_for_context(plugin, frame, context)
            .err()
            .map(|_| IngestRefusal::UnknownFrame)
    };
    if let Some(refusal) = refusal {
        revoke_context_media(plugin, frame, context, &handle.handle);
        return Err(refusal);
    }
    target.retain();
    Ok(IngestedInput {
        handle: handle.handle,
        file_name: handle.file_name,
        content_type: handle.content_type,
        size,
        sha256,
    })
}

pub fn open_context_input(
    plugin: &str,
    context: &crate::view_host::ViewCallContext,
    handle: &str,
) -> Result<OpenedInput, MediaRefusal> {
    if context.plugin != plugin || context.frame.is_none() {
        return Err(MediaRefusal::Foreign);
    }
    let frame = {
        let store = STORE.lock();
        store
            .frames
            .iter()
            .find(|(_, frame)| frame.plugin == plugin && frame.handles.contains_key(handle))
            .map(|(id, _)| id.clone())
            .ok_or(MediaRefusal::Unknown)?
    };
    let registered = super::frame_context(plugin, &frame)?;
    if &registered != context {
        return Err(MediaRefusal::Foreign);
    }
    let sha256 = STORE
        .lock()
        .entry(plugin, &frame, handle, Instant::now())?
        .input_sha256
        .clone()
        .ok_or(MediaRefusal::Foreign)?;
    let media = open_frame_media(plugin, &frame, handle)?;
    Ok(OpenedInput { media, sha256 })
}
#[cfg(test)]
mod tests;
