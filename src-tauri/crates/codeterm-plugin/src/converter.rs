//! Conversion as a plugin capability. `Converter` is a core-owned trait
//! resolved through [`ConverterRegistry`] by declared media kind — never a
//! hardcoded provider match: core turns a file of one media kind into another
//! without knowing which plugin does it.

use parking_lot::{Condvar, Mutex};
use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use codeterm_blueprint_types::sentinel_input::MediaKind;

use crate::transcribe::{TranscribeError, Transcriber};
use futures_util::FutureExt;

#[derive(Default)]
struct ConversionCancellationState {
    cancelled: AtomicBool,
    generation: Mutex<u64>,
    changed: Condvar,
    notify: tokio::sync::Notify,
}

#[derive(Clone, Default)]
pub struct ConversionCancellation(Arc<ConversionCancellationState>);

impl ConversionCancellation {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.notify_change();
        self.0.notify.notify_one();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    pub fn generation(&self) -> u64 {
        *self.0.generation.lock()
    }

    pub fn notify_change(&self) {
        let mut generation = self.0.generation.lock();
        *generation = generation.wrapping_add(1);
        self.0.changed.notify_all();
    }

    pub fn wait_for_change(&self, generation: u64, timeout: std::time::Duration) {
        let mut current = self.0.generation.lock();
        if *current == generation {
            self.0.changed.wait_for(&mut current, timeout);
        }
    }

    async fn cancelled(&self) {
        loop {
            let notified = self.0.notify.notified();
            tokio::pin!(notified);
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// Longest chain of declared edges the planner will walk.
pub const MAX_CONVERSION_STEPS: usize = 3;

/// One declared edge: a converter turns `from` into `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Conversion {
    pub from: MediaKind,
    pub to: MediaKind,
}

impl Conversion {
    pub fn new(from: MediaKind, to: MediaKind) -> Self {
        Self { from, to }
    }
}

/// What a conversion consumes and where its result may land.
pub struct ConvertRequest {
    pub input_path: PathBuf,
    pub from: MediaKind,
    pub to: MediaKind,
    /// Directory the produced file must stay inside.
    pub output_dir: PathBuf,
    pub language: Option<String>,
    /// Point after which the conversion must give up.
    pub deadline: Instant,
    pub cancellation: ConversionCancellation,
}

/// A finished conversion: the text itself, or a file inside `output_dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertOutput {
    Text(String),
    File(PathBuf),
}

/// Failure modes shared by every converter implementation.
#[derive(Debug)]
pub enum ConvertError {
    /// The request is unusable (missing input, kinds not declared, ...).
    InvalidRequest(String),
    /// The converter could not perform the conversion.
    RequestFailed(String),
    /// A returned file lies outside `output_dir`.
    OutputOutsideDir(PathBuf),
    /// The converter produced neither text nor a file.
    NoOutput,
    /// The deadline passed before the conversion completed.
    DeadlineExceeded,
    Cancelled,
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(e) => write!(f, "invalid convert request: {e}"),
            Self::RequestFailed(e) => write!(f, "convert request failed: {e}"),
            Self::OutputOutsideDir(p) => {
                write!(
                    f,
                    "converted file outside the output directory: {}",
                    p.display()
                )
            }
            Self::NoOutput => write!(f, "empty conversion result"),
            Self::DeadlineExceeded => write!(f, "convert deadline exceeded"),
            Self::Cancelled => write!(f, "convert cancelled"),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<TranscribeError> for ConvertError {
    fn from(e: TranscribeError) -> Self {
        match e {
            TranscribeError::Empty | TranscribeError::EmptyPath => Self::NoOutput,
            other => Self::RequestFailed(other.to_string()),
        }
    }
}

/// A conversion backend. Implemented by JS plugins (`JsConverter`) and by
/// adapters over other capabilities (`TranscriberConverter`).
pub trait Converter: Send + Sync {
    /// The edges this converter serves. A step may only ask for one of these.
    fn conversions(&self) -> &[Conversion];
    fn convert(&self, req: &ConvertRequest) -> Result<ConvertOutput, ConvertError>;
}

/// Identity of the converter behind one declared edge. Opaque on purpose:
/// planning is a pure function of declared edges and their order, so no code
/// path can branch on who provides a conversion.
#[derive(Clone)]
pub struct ConverterKey(Arc<dyn Converter>);

impl ConverterKey {
    pub fn new(converter: Arc<dyn Converter>) -> Self {
        Self(converter)
    }

    pub fn conversions(&self) -> &[Conversion] {
        self.0.conversions()
    }

    pub fn convert(&self, req: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
        self.0.convert(req)
    }

    /// Whether both keys are the same registered converter.
    pub fn same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ConverterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ConverterKey")
            .field(&self.0.conversions())
            .finish()
    }
}

/// One planned step: nothing to run, or one converter invocation.
#[derive(Clone, Debug)]
pub enum Step {
    /// `from == to == text`: there is nothing to convert.
    Identity,
    /// One invocation of `key`, which declares `conversion`.
    Convert {
        key: ConverterKey,
        conversion: Conversion,
    },
}

impl PartialEq for Step {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Identity, Self::Identity) => true,
            (
                Self::Convert {
                    key: a,
                    conversion: ca,
                },
                Self::Convert {
                    key: b,
                    conversion: cb,
                },
            ) => a.same_as(b) && ca == cb,
            _ => false,
        }
    }
}

/// No chain of declared edges connects `from` to `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoConverter {
    pub from: MediaKind,
    pub to: MediaKind,
}

impl std::fmt::Display for NoConverter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no converter from {} to {}",
            self.from.as_str(),
            self.to.as_str()
        )
    }
}

impl std::error::Error for NoConverter {}

/// Pure planner over declared edges. Rules, in order:
/// 1. `text -> text` is identity;
/// 2. a direct edge beats any chain;
/// 3. otherwise the shortest chain of declared edges, at most
///    [`MAX_CONVERSION_STEPS`] steps.
/// Ties resolve by the order of `available` (registration order), never by any
/// converter identity.
pub fn plan_conversion(
    available: &[(ConverterKey, Conversion)],
    from: MediaKind,
    to: MediaKind,
) -> Result<Vec<Step>, NoConverter> {
    if from == to {
        return if from == MediaKind::Text {
            Ok(vec![Step::Identity])
        } else {
            Err(NoConverter { from, to })
        };
    }
    // Breadth-first over media kinds, edges considered in `available` order, so
    // the first path found is both the shortest and the earliest registered one.
    let mut reached: std::collections::BTreeMap<MediaKind, Vec<usize>> =
        std::collections::BTreeMap::new();
    let mut queue: VecDeque<MediaKind> = VecDeque::new();
    reached.insert(from, Vec::new());
    queue.push_back(from);
    while let Some(kind) = queue.pop_front() {
        let path = reached[&kind].clone();
        if path.len() >= MAX_CONVERSION_STEPS {
            continue;
        }
        for (idx, (_key, edge)) in available.iter().enumerate() {
            if edge.from != kind {
                continue;
            }
            if edge.to == to {
                let mut chain = path.clone();
                chain.push(idx);
                return Ok(chain
                    .into_iter()
                    .map(|i| Step::Convert {
                        key: available[i].0.clone(),
                        conversion: available[i].1,
                    })
                    .collect());
            }
            if reached.contains_key(&edge.to) {
                continue;
            }
            let mut chain = path.clone();
            chain.push(idx);
            reached.insert(edge.to, chain);
            queue.push_back(edge.to);
        }
    }
    Err(NoConverter { from, to })
}

/// Registered converters in stable registration order. Entries are keyed by
/// registration id for lifecycle only (unregister across reloads); one plugin
/// may contribute several converters (its declared ones plus a derived
/// adapter). What a conversion can do is a pure function of the declared
/// [`Conversion`] edges — resolution never consults who registered them.
pub struct ConverterRegistry {
    entries: parking_lot::RwLock<Vec<Entry>>,
}

struct Entry {
    id: String,
    converters: Vec<Arc<dyn Converter>>,
}

impl ConverterRegistry {
    pub fn new() -> Self {
        Self {
            entries: parking_lot::RwLock::new(Vec::new()),
        }
    }

    /// Register one converter under `id`, into that id's existing slot so
    /// ordering stays stable; the reload path calls [`Self::unregister`] first,
    /// which clears every converter the id contributed.
    pub fn register(&self, id: impl Into<String>, converter: Arc<dyn Converter>) {
        let id = id.into();
        let mut entries = self.entries.write();
        match entries.iter_mut().find(|entry| entry.id == id) {
            Some(entry) => entry.converters.push(converter),
            None => entries.push(Entry {
                id,
                converters: vec![converter],
            }),
        }
    }

    /// Remove every converter `id` contributed, if present.
    pub fn unregister(&self, id: &str) {
        self.entries.write().retain(|entry| entry.id.as_str() != id);
    }

    /// The converters `id` contributed (empty when it registered none).
    pub fn converters_for(&self, id: &str) -> Vec<Arc<dyn Converter>> {
        self.entries
            .read()
            .iter()
            .find(|entry| entry.id.as_str() == id)
            .map(|entry| entry.converters.clone())
            .unwrap_or_default()
    }

    /// Every declared edge paired with its converter, in registration order.
    pub fn available(&self) -> Vec<(ConverterKey, Conversion)> {
        self.entries
            .read()
            .iter()
            .flat_map(|entry| {
                entry.converters.iter().flat_map(|converter| {
                    converter
                        .conversions()
                        .iter()
                        .map(|conversion| (ConverterKey::new(converter.clone()), *conversion))
                        .collect::<Vec<_>>()
                })
            })
            .collect()
    }

    /// Plan `from` -> `to` over the currently registered edges.
    pub fn plan(&self, from: MediaKind, to: MediaKind) -> Result<Vec<Step>, NoConverter> {
        plan_conversion(&self.available(), from, to)
    }
}

impl Default for ConverterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether `path` lands strictly inside `dir` after lexical normalization, so a
/// returned path with `..` components cannot escape the output directory.
pub fn output_within(dir: &Path, path: &Path) -> bool {
    let dir = lexical_normalize(dir);
    let path = lexical_normalize(path);
    path.starts_with(&dir) && path.components().count() > dir.components().count()
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Wraps a [`Transcriber`] as the audio -> text edge of the conversion seam, so
/// a transcription-only plugin chains like any converter without exporting
/// `convert`.
pub struct TranscriberConverter {
    inner: Arc<dyn Transcriber>,
    conversions: [Conversion; 1],
}

impl TranscriberConverter {
    pub fn new(inner: Arc<dyn Transcriber>) -> Self {
        Self {
            inner,
            conversions: [Conversion::new(MediaKind::Audio, MediaKind::Text)],
        }
    }
}

impl Converter for TranscriberConverter {
    fn conversions(&self) -> &[Conversion] {
        &self.conversions
    }

    fn convert(&self, req: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
        if req.from != MediaKind::Audio || req.to != MediaKind::Text {
            return Err(ConvertError::InvalidRequest(format!(
                "declares {} -> {}",
                self.conversions[0].from.as_str(),
                self.conversions[0].to.as_str()
            )));
        }
        if Instant::now() >= req.deadline {
            return Err(ConvertError::DeadlineExceeded);
        }
        if req.cancellation.is_cancelled() {
            return Err(ConvertError::Cancelled);
        }
        let language = req.language.as_deref().unwrap_or("");
        let transcript = futures_executor::block_on(async {
            let transcribe = self.inner.transcribe(&req.input_path, language).fuse();
            let cancelled = req.cancellation.cancelled().fuse();
            futures_util::pin_mut!(transcribe, cancelled);
            match futures_util::future::select(transcribe, cancelled).await {
                futures_util::future::Either::Left((result, _)) => {
                    result.map_err(ConvertError::from)
                }
                futures_util::future::Either::Right(_) => Err(ConvertError::Cancelled),
            }
        })?;
        Ok(ConvertOutput::Text(transcript.text))
    }
}

#[cfg(test)]
mod tests;
