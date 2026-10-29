use super::{
    model::*,
    policy::{TransferAbort, TransferSession},
};
use futures_util::{stream, StreamExt};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
pub const CHUNK: usize = 65536;
pub type ByteStream = futures_util::stream::BoxStream<'static, Result<Vec<u8>>>;
pub struct SelectedFile {
    pub(crate) file: File,
    pub(crate) size: u64,
    pub(crate) name: String,
    pub(crate) mime: String,
    pub(crate) sha256: String,
}
impl SelectedFile {
    pub(crate) fn validate(
        mut file: File,
        name: &str,
        mime: &str,
        limit: u64,
        identity: Option<&FileIdentity>,
        check: impl Fn() -> Result<()>,
    ) -> Result<Self> {
        let meta = file.metadata().map_err(|_| TransferRefusal::Storage)?;
        if !meta.is_file() {
            return Err(TransferRefusal::NotAllowed);
        }
        if identity.is_some_and(|identity| !crate::media_delivery::valid_token(&identity.sha256)) {
            return Err(TransferRefusal::InvalidRequest);
        }
        let size = identity.map_or(meta.len(), |identity| identity.size);
        if size > limit {
            return Err(TransferRefusal::TooLarge);
        }
        if meta.len() < size {
            return Err(TransferRefusal::FileChanged);
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| TransferRefusal::Storage)?;
        let mut remaining = size;
        let mut head = Vec::with_capacity(32);
        let mut hash = Sha256::new();
        let mut bytes = vec![0; CHUNK];
        while remaining > 0 {
            check()?;
            let length = remaining.min(CHUNK as u64) as usize;
            file.read_exact(&mut bytes[..length])
                .map_err(|_| TransferRefusal::FileChanged)?;
            let keep = (32 - head.len()).min(length);
            head.extend_from_slice(&bytes[..keep]);
            hash.update(&bytes[..length]);
            remaining -= length as u64;
        }
        validate_mime(mime, &head)?;
        let sha256 = format!("{:x}", hash.finalize());
        if identity.is_some_and(|identity| identity.sha256 != sha256) {
            return Err(TransferRefusal::DigestMismatch);
        }
        check()?;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| TransferRefusal::Storage)?;
        Ok(Self {
            file,
            size,
            name: safe_name(name, mime)?,
            mime: mime.into(),
            sha256,
        })
    }
}
pub fn media_kind(mime: &str) -> Option<crate::media_delivery::MediaType> {
    serde_json::from_value(serde_json::Value::String(mime.into())).ok()
}
pub fn validate_mime(mime: &str, head: &[u8]) -> Result<()> {
    if mime == "application/octet-stream" || media_kind(mime).is_some_and(|kind| kind.matches(head))
    {
        Ok(())
    } else {
        Err(TransferRefusal::MimeMismatch)
    }
}
pub fn safe_name(name: &str, mime: &str) -> Result<String> {
    if name.len() > 512 {
        return Err(TransferRefusal::InvalidRequest);
    }
    if let Some(kind) = media_kind(mime) {
        return Ok(crate::media_delivery::safe_transfer_name(name, Some(kind)));
    }
    if mime == "application/octet-stream" {
        return Ok(crate::media_delivery::safe_transfer_name(name, None));
    }
    Err(TransferRefusal::MimeMismatch)
}
fn field(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        Err(TransferRefusal::InvalidRequest)
    } else {
        Ok(())
    }
}
enum Segment {
    Bytes(std::io::Cursor<Vec<u8>>),
    File {
        file: File,
        remaining: u64,
        hash: Sha256,
        expected: String,
    },
}
fn file_segment(source: SelectedFile) -> Segment {
    Segment::File {
        file: source.file,
        remaining: source.size,
        hash: Sha256::new(),
        expected: source.sha256,
    }
}
#[derive(Clone)]
pub struct UploadState {
    expected: u64,
    sent: Arc<AtomicU64>,
    failure: Arc<Mutex<Option<TransferRefusal>>>,
}
impl UploadState {
    pub fn finish(&self) -> Result<()> {
        if let Some(error) = *self.failure.lock() {
            return Err(error);
        }
        if self.sent.load(Ordering::Acquire) != self.expected {
            return Err(TransferRefusal::TransportUnconfirmed);
        }
        Ok(())
    }
    pub(crate) fn failure(&self) -> Option<TransferRefusal> {
        *self.failure.lock()
    }
}

pub struct Upload {
    pub length: u64,
    pub content_type: Option<String>,
    pub stream: ByteStream,
    pub state: UploadState,
}
fn input(
    session: &TransferSession,
    value: Input,
    origin: &reqwest::Url,
    check: impl Fn() -> Result<()>,
) -> Result<SelectedFile> {
    match value {
        Input::Selected { handle } => {
            let source = session.take_input(&handle, origin)?;
            let identity = FileIdentity {
                size: source.size,
                sha256: source.sha256,
            };
            SelectedFile::validate(
                source.file,
                &source.name,
                &source.mime,
                session.limits.max_upload_bytes,
                Some(&identity),
                check,
            )
        }
        Input::Owned {
            path,
            content_type,
            file_name,
            identity,
        } => {
            if path.len() > 4096
                || path.contains(['\\', ':', '\0'])
                || !session.authority.readable(&session.root.join(&path))
            {
                return Err(TransferRefusal::NotAllowed);
            }
            let file = codeterm_blueprint_types::code_source::open_file(&session.root, &path)
                .map_err(|_| TransferRefusal::NotAllowed)?;
            SelectedFile::validate(
                file,
                &file_name,
                &content_type,
                session.limits.max_upload_bytes,
                identity.as_ref(),
                check,
            )
        }
    }
}
pub fn build(
    session: Arc<TransferSession>,
    origin: &reqwest::Url,
    body: Option<InputBody>,
    abort: TransferAbort,
    deadline: std::time::Instant,
) -> Result<Upload> {
    let mut segments = VecDeque::new();
    let mut length = 0u64;
    let mut content_type = None;
    let mut append = |segment: Segment, size: u64| -> Result<()> {
        length = length.checked_add(size).ok_or(TransferRefusal::TooLarge)?;
        if length > session.limits.max_upload_bytes {
            return Err(TransferRefusal::TooLarge);
        }
        segments.push_back(segment);
        Ok(())
    };
    match body {
        None => {}
        Some(InputBody::File { file }) => {
            let source = input(&session, file, origin, || {
                if abort.is_aborted() {
                    return Err(TransferRefusal::Aborted);
                }
                if std::time::Instant::now() >= deadline {
                    return Err(TransferRefusal::Deadline);
                }
                session.authorize(origin)
            })?;
            let size = source.size;
            content_type = Some(source.mime.clone());
            append(file_segment(source), size)?;
        }
        Some(InputBody::Multipart { parts }) => {
            if parts.is_empty() || parts.len() > 16 {
                return Err(TransferRefusal::InvalidRequest);
            }
            let boundary = format!("domios-{}", uuid::Uuid::new_v4().simple());
            content_type = Some(format!("multipart/form-data; boundary={boundary}"));
            let mut text_bytes = 0usize;
            for part in parts {
                let (head, bytes, source) = match part {
                    Part::Text { name, value } => {
                        field(&name)?;
                        text_bytes = text_bytes.saturating_add(value.len());
                        if text_bytes > 65536 {
                            return Err(TransferRefusal::TooLarge);
                        }
                        (format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n"),Some(value.into_bytes()),None)
                    }
                    Part::File { name, file } => {
                        field(&name)?;
                        let source = input(&session, file, origin, || {
                            if abort.is_aborted() {
                                return Err(TransferRefusal::Aborted);
                            }
                            if std::time::Instant::now() >= deadline {
                                return Err(TransferRefusal::Deadline);
                            }
                            session.authorize(origin)
                        })?;
                        let head=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",source.name,source.mime);
                        (head, None, Some(source))
                    }
                };
                let size = head.len() as u64;
                append(
                    Segment::Bytes(std::io::Cursor::new(head.into_bytes())),
                    size,
                )?;
                if let Some(bytes) = bytes {
                    let size = bytes.len() as u64;
                    append(Segment::Bytes(std::io::Cursor::new(bytes)), size)?;
                }
                if let Some(source) = source {
                    let size = source.size;
                    append(file_segment(source), size)?;
                }
                append(Segment::Bytes(std::io::Cursor::new(b"\r\n".to_vec())), 2)?;
            }
            let end = format!("--{boundary}--\r\n").into_bytes();
            let size = end.len() as u64;
            append(Segment::Bytes(std::io::Cursor::new(end)), size)?;
        }
    }
    let state = UploadState {
        expected: length,
        sent: Arc::new(AtomicU64::new(0)),
        failure: Arc::new(Mutex::new(None)),
    };
    let progress = state.clone();
    let stream = stream::try_unfold(
        (segments, session, origin.clone(), abort),
        move |(mut segments, session, origin, abort)| async move {
            loop {
                if abort.is_aborted() {
                    return Err(TransferRefusal::Aborted);
                }
                if std::time::Instant::now() >= deadline {
                    return Err(TransferRefusal::Deadline);
                }
                session.authorize(&origin)?;
                let Some(segment) = segments.front_mut() else {
                    return Ok(None);
                };
                let mut bytes = vec![0; CHUNK];
                let count = match segment {
                    Segment::Bytes(cursor) => cursor
                        .read(&mut bytes)
                        .map_err(|_| TransferRefusal::Storage)?,
                    Segment::File {
                        file,
                        remaining,
                        hash,
                        expected,
                    } => {
                        if *remaining == 0 {
                            segments.pop_front();
                            continue;
                        }
                        let length = (*remaining).min(CHUNK as u64) as usize;
                        let count = file
                            .read(&mut bytes[..length])
                            .map_err(|_| TransferRefusal::Storage)?;
                        if count == 0 {
                            return Err(TransferRefusal::FileChanged);
                        }
                        *remaining -= count as u64;
                        hash.update(&bytes[..count]);
                        if *remaining == 0 && format!("{:x}", hash.clone().finalize()) != *expected
                        {
                            return Err(TransferRefusal::DigestMismatch);
                        }
                        count
                    }
                };
                if count == 0 {
                    segments.pop_front();
                    continue;
                }
                bytes.truncate(count);
                return Ok(Some((bytes, (segments, session, origin, abort))));
            }
        },
    )
    .inspect(move |chunk| match chunk {
        Ok(bytes) => {
            progress
                .sent
                .fetch_add(bytes.len() as u64, Ordering::AcqRel);
        }
        Err(error) => {
            *progress.failure.lock() = Some(*error);
        }
    })
    .boxed();
    Ok(Upload {
        length,
        content_type,
        stream,
        state,
    })
}
