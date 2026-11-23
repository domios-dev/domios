use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use codeterm_util::private_file;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::binding::BindingDocument;
use super::{
    AppendReceipt, TranscriptContent, TranscriptInput, TranscriptRange, TranscriptRow,
    TranscriptSeq,
};

pub const RAW_SEGMENT_BYTES: u64 = 2 * 1024 * 1024;
pub const RAW_SEGMENT_ROWS: usize = 1000;
pub const MAX_ROW_BYTES: usize = 64 * 1024;
const RECENT_FORWARDS: usize = 50;
pub(super) const MAX_RECORD_BYTES: u64 = 128 * 1024;
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct TranscriptStore {
    pub(super) inner: Arc<StoreInner>,
}

pub(super) struct StoreInner {
    pub(super) root: PathBuf,
    pub(super) state: Mutex<StoreState>,
    pub(super) raw_budget_bytes: u64,
    pub(super) forward_gate: Mutex<()>,
    _lease: File,
}

pub(super) struct StoreState {
    pub(super) manifest: Manifest,
    pub(super) binding: BindingDocument,
    pub(super) segments: Vec<Segment>,
    pub(super) action_ids: Vec<String>,
    forward_paths: BTreeSet<PathBuf>,
    pub(super) readers: usize,
    write_uncertain: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptForward {
    pub forward_id: String,
    pub epoch_id: Option<String>,
    pub range: TranscriptRange,
    pub at_ms: u64,
    pub target_tab_id: String,
    pub target_title: String,
    pub sent_text: String,
    pub sent_excerpt: String,
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Manifest {
    schema_version: u32,
    owner_tab_id: String,
    sealed_through: TranscriptSeq,
    pub(super) cold_ranges: BTreeMap<TranscriptSeq, TranscriptRange>,
}

#[derive(Debug)]
pub(super) struct Segment {
    pub(super) range: TranscriptRange,
    pub(super) bytes: u64,
    pub(super) summarized: bool,
    rows: usize,
}

impl Segment {
    pub(super) fn has_raw(&self) -> bool {
        self.bytes != 0
    }
}

impl StoreState {
    pub(super) fn last_seq(&self) -> TranscriptSeq {
        self.segments
            .last()
            .map_or(0, |segment| segment.range.to_seq)
    }

    pub(super) fn segment_index(&self, seq: TranscriptSeq) -> usize {
        self.segments
            .partition_point(|segment| segment.range.to_seq < seq)
    }

    pub(super) fn is_sealed(&self, index: usize) -> bool {
        let segment = &self.segments[index];
        !segment.has_raw()
            || index + 1 < self.segments.len()
            || segment.range.to_seq <= self.manifest.sealed_through
            || segment.rows == RAW_SEGMENT_ROWS
            || segment.bytes == RAW_SEGMENT_BYTES
    }

    pub(super) fn require_healthy(&self) -> io::Result<()> {
        if self.write_uncertain {
            return Err(io::Error::other("Reopen transcript after uncertain write"));
        }
        Ok(())
    }

    pub(super) fn fence_writes(&mut self) {
        self.write_uncertain = true;
    }
}

impl TranscriptStore {
    /// The host supplies an authorized original owner and shares this handle across services.
    /// All filesystem operations belong on a blocking worker.
    pub fn open(runtime_dir: &Path, owner_tab_id: &str, raw_budget_bytes: u64) -> io::Result<Self> {
        validate_tab_id(owner_tab_id)?;
        let runtime_dir = runtime_dir.canonicalize()?;
        let transcripts = private_dir(&runtime_dir, "transcripts")?;
        let root = private_dir(&transcripts, owner_tab_id)?;
        let lock_path = root.join("writer.lock");
        let lease = match private_file::create_new_owner_only(&lock_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                open_private(&lock_path, 0)?
            }
            Err(error) => return Err(error),
        };
        lease.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => {
                io::Error::new(io::ErrorKind::WouldBlock, "Transcript store already open")
            }
            fs::TryLockError::Error(error) => error,
        })?;

        for directory in [
            "raw",
            "summaries",
            "cursors",
            "concierge_epochs",
            "route_actions",
            "forwards",
        ] {
            private_dir(&root, directory)?;
        }
        let manifest_path = root.join("manifest.json");
        let manifest: Manifest = match read_private(&manifest_path, MAX_MANIFEST_BYTES)? {
            Some(manifest) => manifest,
            None => {
                let manifest = Manifest {
                    schema_version: 1,
                    owner_tab_id: owner_tab_id.to_owned(),
                    sealed_through: 0,
                    cold_ranges: BTreeMap::new(),
                };
                write_private(&manifest_path, &manifest, MAX_MANIFEST_BYTES)?;
                manifest
            }
        };
        if manifest.schema_version != 1 || manifest.owner_tab_id != owner_tab_id {
            return Err(invalid_data("Transcript manifest identity mismatch"));
        }

        let segments = recover_segments(&root, &manifest)?;
        let last_seq = segments.last().map_or(0, |segment| segment.range.to_seq);
        if manifest.sealed_through != 0
            && segments
                .binary_search_by_key(&manifest.sealed_through, |segment| segment.range.to_seq)
                .is_err()
        {
            return Err(invalid_data(
                "Transcript seal does not match committed history",
            ));
        }
        let binding = BindingDocument::load(&root, owner_tab_id, last_seq)?;
        let action_ids = super::binding::recover_actions(&root, &binding.binding, last_seq)?;
        let mut forward_paths = BTreeSet::new();
        for entry in fs::read_dir(root.join("forwards"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                insert_forward_path(&mut forward_paths, path);
            }
        }
        Ok(Self {
            inner: Arc::new(StoreInner {
                root,
                state: Mutex::new(StoreState {
                    manifest,
                    binding,
                    segments,
                    action_ids,
                    forward_paths,
                    readers: 0,
                    write_uncertain: false,
                }),
                raw_budget_bytes,
                forward_gate: Mutex::new(()),
                _lease: lease,
            }),
        })
    }

    pub fn committed_seq(&self) -> TranscriptSeq {
        self.inner.state.lock().last_seq()
    }

    pub fn record_forward(&self, forward: TranscriptForward) -> io::Result<()> {
        validate_record_id(&forward.forward_id)?;
        validate_tab_id(&forward.target_tab_id)?;
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        let range = forward.range;
        if range.from_seq == 0 || range.from_seq > range.to_seq || range.to_seq > state.last_seq() {
            return Err(invalid_input("Forward range exceeds committed transcript"));
        }
        let path = forward_path(&self.inner.root, &forward);
        write_private(&path, &forward, MAX_RECORD_BYTES)?;
        insert_forward_path(&mut state.forward_paths, path);
        Ok(())
    }

    pub fn set_forward_feedback(
        &self,
        forward_id: &str,
        verdict: &str,
    ) -> io::Result<TranscriptForward> {
        self.update_forward_feedback(forward_id, verdict, |_| Ok(()))
    }

    pub(super) fn update_forward_feedback(
        &self,
        forward_id: &str,
        verdict: &str,
        after_write: impl FnOnce(&TranscriptForward) -> io::Result<()>,
    ) -> io::Result<TranscriptForward> {
        validate_record_id(forward_id)?;
        if !matches!(verdict, "wrong_send" | "good_send") {
            return Err(invalid_input("Expected wrong_send or good_send feedback"));
        }
        let _gate = self.inner.forward_gate.lock();
        let state = self.inner.state.lock();
        state.require_healthy()?;
        let dir = self.inner.root.join("forwards");
        let encoded_suffix = encoded_record_path(Path::new(""), &format!("-{forward_id}"));
        let suffix = encoded_suffix.to_string_lossy();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if !path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.len() == 40 + suffix.len() && name.ends_with(suffix.as_ref())
            }) {
                continue;
            }
            let mut forward: TranscriptForward = read_private(&path, MAX_RECORD_BYTES)?
                .ok_or_else(|| invalid_data("Transcript forward is missing"))?;
            if forward.forward_id != forward_id || forward_path(&self.inner.root, &forward) != path
            {
                return Err(invalid_data("Transcript forward identity mismatch"));
            }
            let previous = forward.feedback.replace(verdict.to_owned());
            write_private(&path, &forward, MAX_RECORD_BYTES)?;
            if let Err(error) = after_write(&forward) {
                forward.feedback = previous;
                write_private(&path, &forward, MAX_RECORD_BYTES)?;
                return Err(error);
            }
            return Ok(forward);
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Transcript forward is missing",
        ))
    }

    pub fn forwards(&self) -> io::Result<Vec<TranscriptForward>> {
        let paths = self
            .inner
            .state
            .lock()
            .forward_paths
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        paths
            .iter()
            .map(|path| {
                let forward = read_private::<TranscriptForward>(path, MAX_RECORD_BYTES)?
                    .ok_or_else(|| invalid_data("Transcript forward is missing"))?;
                if forward_path(&self.inner.root, &forward) != *path {
                    return Err(invalid_data("Transcript forward identity mismatch"));
                }
                Ok(forward)
            })
            .collect()
    }

    /// A successful receipt is the publication boundary for committed speech and seal events.
    /// After a write error, reopen and reconcile the prefix before submitting more input.
    pub fn append(&self, live_tab_id: &str, input: TranscriptInput) -> io::Result<AppendReceipt> {
        let mut state = self.inner.state.lock();
        state.binding.require_live_owner(live_tab_id)?;
        state.require_healthy()?;
        let row = TranscriptRow {
            seq: next_seq(state.last_seq())?,
            at_ms: input.at_ms,
            content: input.content,
        };
        next_seq(row.seq)?;
        validate_row_content(&row.content)?;
        let mut bytes = serde_json::to_vec(&row)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_ROW_BYTES {
            return Err(invalid_input("Encoded transcript row exceeds byte limit"));
        }

        let mut sealed = Vec::new();
        let mut index = active_index(&state);
        if let Some(active) = index {
            if state.segments[active].bytes + bytes.len() as u64 > RAW_SEGMENT_BYTES {
                sealed.push(state.segments[active].range);
                index = None;
            }
        }

        let first_seq = index.map_or(row.seq, |index| state.segments[index].range.from_seq);
        let path = raw_path(&self.inner.root, first_seq);
        let write_result = if let Some(index) = index {
            append_bytes(&path, state.segments[index].bytes, &bytes, false)
        } else {
            append_bytes(&path, 0, &bytes, true)
        };
        if let Err(error) = write_result {
            state.fence_writes();
            return Err(error);
        }

        if let Some(index) = index {
            let segment = &mut state.segments[index];
            segment.range.to_seq = row.seq;
            segment.bytes += bytes.len() as u64;
            segment.rows += 1;
        } else {
            state.segments.push(Segment {
                range: TranscriptRange {
                    from_seq: row.seq,
                    to_seq: row.seq,
                },
                bytes: bytes.len() as u64,
                summarized: false,
                rows: 1,
            });
        }
        let index = state.segments.len() - 1;
        if state.is_sealed(index) {
            sealed.push(state.segments[index].range);
        }
        Ok(AppendReceipt { row, sealed })
    }

    pub fn seal_active(&self, live_tab_id: &str) -> io::Result<Option<TranscriptRange>> {
        let mut state = self.inner.state.lock();
        state.binding.require_live_owner(live_tab_id)?;
        state.require_healthy()?;
        active_index(&state)
            .map(|index| self.seal_locked(&mut state, index))
            .transpose()
    }

    pub(super) fn seal_locked(
        &self,
        state: &mut StoreState,
        index: usize,
    ) -> io::Result<TranscriptRange> {
        state.require_healthy()?;
        let range = state.segments[index].range;
        let mut manifest = state.manifest.clone();
        manifest.sealed_through = range.to_seq;
        self.save_manifest(state, manifest)?;
        Ok(range)
    }

    pub(super) fn save_manifest(
        &self,
        state: &mut StoreState,
        manifest: Manifest,
    ) -> io::Result<()> {
        if let Err(error) = write_private(
            &self.inner.root.join("manifest.json"),
            &manifest,
            MAX_MANIFEST_BYTES,
        ) {
            state.fence_writes();
            return Err(error);
        }
        state.manifest = manifest;
        Ok(())
    }
}

fn forward_path(root: &Path, forward: &TranscriptForward) -> PathBuf {
    encoded_record_path(
        &root.join("forwards"),
        &format!("{:020}-{}", forward.at_ms, forward.forward_id),
    )
}

fn insert_forward_path(paths: &mut BTreeSet<PathBuf>, path: PathBuf) {
    paths.insert(path);
    if paths.len() > RECENT_FORWARDS {
        paths.pop_first();
    }
}

pub(super) fn active_index(state: &StoreState) -> Option<usize> {
    state
        .segments
        .len()
        .checked_sub(1)
        .filter(|index| !state.is_sealed(*index))
}

pub(super) fn next_seq(seq: TranscriptSeq) -> io::Result<TranscriptSeq> {
    seq.checked_add(1)
        .ok_or_else(|| invalid_data("Transcript sequence exhausted"))
}

fn recover_segments(root: &Path, manifest: &Manifest) -> io::Result<Vec<Segment>> {
    let mut raw = BTreeMap::new();
    for entry in fs::read_dir(root.join("raw"))? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(first_seq) = name
            .to_str()
            .and_then(|name| name.strip_suffix(".jsonl"))
            .and_then(|seq| seq.parse::<TranscriptSeq>().ok())
            .filter(|seq| *seq != 0 && name == raw_name(*seq).as_str())
        else {
            return Err(invalid_data("Unexpected transcript segment name"));
        };
        raw.insert(first_seq, entry.path());
    }

    let mut starts = raw
        .keys()
        .chain(manifest.cold_ranges.keys())
        .copied()
        .collect::<Vec<_>>();
    starts.sort_unstable();
    starts.dedup();
    let mut segments = Vec::new();
    let mut expected = 1;
    let tail_start = starts.last().copied();
    for first_seq in starts {
        if first_seq != expected {
            return Err(invalid_data("Gap or overlap in transcript segment index"));
        }
        let cold = manifest.cold_ranges.get(&first_seq).copied();
        if let Some(range) = cold {
            if range.from_seq != first_seq || range.to_seq < first_seq {
                return Err(invalid_data("Invalid cold transcript range"));
            }
            super::retention::require_summary(root, range)?;
        }
        let Some(path) = raw.remove(&first_seq) else {
            let range = cold.ok_or_else(|| invalid_data("Missing transcript segment"))?;
            expected = next_seq(range.to_seq)?;
            segments.push(Segment {
                range,
                bytes: 0,
                rows: 0,
                summarized: true,
            });
            continue;
        };
        let may_repair = Some(first_seq) == tail_start && cold.is_none();
        let (mut segment, original_len) = scan_raw(&path, first_seq, may_repair)?;
        if let Some(range) = cold {
            if segment.range != range {
                return Err(invalid_data("Cold range does not match retained raw"));
            }
        }
        if segment.rows == 0 {
            fs::remove_file(&path)?;
            sync_parent(&path)?;
            continue;
        }
        if segment.bytes != original_len {
            let tail = open_raw_write(&path, original_len)?;
            tail.set_len(segment.bytes)?;
            tail.sync_all()?;
        }
        segment.summarized =
            cold.is_some() || super::retention::read_summary(root, segment.range)?.is_some();
        expected = next_seq(segment.range.to_seq)?;
        segments.push(segment);
    }
    Ok(segments)
}

fn scan_raw(path: &Path, first_seq: TranscriptSeq, may_repair: bool) -> io::Result<(Segment, u64)> {
    let file = open_private(path, RAW_SEGMENT_BYTES)?;
    let original_len = file.metadata()?.len();
    let mut input = BufReader::new(file);
    let mut line = Vec::new();
    let mut bytes = 0;
    let mut rows = 0;
    let mut expected = first_seq;

    loop {
        match read_line(&mut input, &mut line) {
            Ok(false) => break,
            Ok(true) => {}
            Err(error) if may_repair && error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        }
        let row: TranscriptRow = serde_json::from_slice(&line)?;
        if row.seq != expected || rows == RAW_SEGMENT_ROWS {
            return Err(invalid_data("Invalid committed transcript sequence"));
        }
        validate_row_content(&row.content)?;
        bytes += line.len() as u64;
        rows += 1;
        expected = next_seq(expected)?;
    }
    if rows == 0 && !may_repair {
        return Err(invalid_data(
            "Empty transcript segment before committed history",
        ));
    }
    Ok((
        Segment {
            range: TranscriptRange {
                from_seq: first_seq,
                to_seq: expected - 1,
            },
            rows,
            bytes,
            summarized: false,
        },
        original_len,
    ))
}

pub(super) fn visit_raw(
    root: &Path,
    segment: &Segment,
    mut visit: impl FnMut(TranscriptRow) -> io::Result<bool>,
) -> io::Result<()> {
    let file = open_private(&raw_path(root, segment.range.from_seq), RAW_SEGMENT_BYTES)?;
    if file.metadata()?.len() != segment.bytes {
        return Err(invalid_data(
            "Raw transcript length changed outside the store",
        ));
    }
    let mut input = BufReader::new(file.take(segment.bytes));
    let mut line = Vec::new();
    let mut next = segment.range.from_seq;
    while read_line(&mut input, &mut line)? {
        let row: TranscriptRow = serde_json::from_slice(&line)?;
        if row.seq != next || row.seq > segment.range.to_seq {
            return Err(invalid_data(
                "Raw transcript sequence changed outside the store",
            ));
        }
        next = next_seq(row.seq)?;
        if !visit(row)? {
            return Ok(());
        }
    }
    if next <= segment.range.to_seq {
        return Err(invalid_data(
            "Raw transcript ended before its committed frontier",
        ));
    }
    Ok(())
}

fn read_line(input: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<bool> {
    line.clear();
    if input
        .take(MAX_ROW_BYTES as u64 + 1)
        .read_until(b'\n', line)?
        == 0
    {
        return Ok(false);
    }
    if line.len() > MAX_ROW_BYTES {
        return Err(invalid_data("Transcript row exceeds byte limit"));
    }
    if line.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Torn transcript row",
        ));
    }
    Ok(true)
}

fn validate_row_content(content: &TranscriptContent) -> io::Result<()> {
    match content {
        TranscriptContent::Speech { text }
            if text.len() > MAX_ROW_BYTES || text.trim().is_empty() =>
        {
            Err(invalid_input("Empty or oversized committed speech"))
        }
        TranscriptContent::Paused { reason } if reason.len() > 1024 => {
            Err(invalid_input("Pause reason exceeds byte limit"))
        }
        _ => Ok(()),
    }
}

fn append_bytes(path: &Path, previous_bytes: u64, bytes: &[u8], is_new: bool) -> io::Result<()> {
    let mut file = if is_new {
        verify_parent(path)?;
        private_file::create_new_owner_only(path)?
    } else {
        open_raw_write(path, previous_bytes)?
    };
    file.seek(SeekFrom::Start(previous_bytes))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if is_new {
        sync_parent(path)?;
    }
    Ok(())
}

fn open_raw_write(path: &Path, expected_bytes: u64) -> io::Result<File> {
    let checked = open_private(path, RAW_SEGMENT_BYTES)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    if file_identity(&checked)? != file_identity(&file)? || file.metadata()?.len() != expected_bytes
    {
        return Err(invalid_data("Raw transcript changed outside the store"));
    }
    Ok(file)
}

pub(super) fn raw_path(root: &Path, first_seq: TranscriptSeq) -> PathBuf {
    root.join("raw").join(raw_name(first_seq))
}

fn raw_name(first_seq: TranscriptSeq) -> String {
    format!("{first_seq:020}.jsonl")
}

pub(super) fn validate_tab_id(id: &str) -> io::Result<()> {
    let windows_device = matches!(id, "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|prefix| {
            id.strip_prefix(prefix)
                .is_some_and(|suffix| suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit())
        });
    if id.is_empty()
        || id.len() > 96
        || windows_device
        || !id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
    {
        return Err(invalid_input("Non-canonical transcript tab identity"));
    }
    Ok(())
}

pub(super) fn encoded_record_path(directory: &Path, key: &str) -> PathBuf {
    use std::fmt::Write as _;
    let mut name = String::with_capacity(key.len() * 2 + 5);
    for byte in key.bytes() {
        let _ = write!(name, "{byte:02x}");
    }
    name.push_str(".json");
    directory.join(name)
}

pub(super) fn validate_record_id(id: &str) -> io::Result<()> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
    {
        return Err(invalid_input("Non-canonical transcript record identity"));
    }
    Ok(())
}

fn private_dir(parent: &Path, name: &str) -> io::Result<PathBuf> {
    let path = parent.join(name);
    match private_file::create_dir_owner_only(&path) {
        Ok(()) => sync_parent(&path)?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    private_file::open_directory_owner_only(&path)?;
    verify_canonical(&path)?;
    Ok(path)
}

fn verify_parent(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_input("Missing transcript directory"))?;
    private_file::open_directory_owner_only(parent)?;
    verify_canonical(parent)
}

fn verify_canonical(path: &Path) -> io::Result<()> {
    let actual = path.canonicalize()?;
    if actual.as_os_str() != path.as_os_str() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Aliased transcript path",
        ));
    }
    Ok(())
}

pub(super) fn open_private(path: &Path, max_bytes: u64) -> io::Result<File> {
    verify_parent(path)?;
    let file = private_file::open_owner_only(path, max_bytes)?;
    verify_canonical(path)?;
    file_identity(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn file_identity(file: &File) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if metadata.nlink() != 1 {
        return Err(invalid_data("Hard-linked transcript file"));
    }
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn file_identity(file: &File) -> io::Result<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
    };
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if information.nNumberOfLinks != 1
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(invalid_data("Aliased transcript file"));
    }
    Ok((
        u64::from(information.dwVolumeSerialNumber),
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    ))
}

pub(super) fn read_private<T: DeserializeOwned>(
    path: &Path,
    max_bytes: u64,
) -> io::Result<Option<T>> {
    let file = match open_private(path, max_bytes) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(invalid_data("Transcript metadata exceeds byte limit"));
    }
    Ok(Some(serde_json::from_slice(&bytes)?))
}

pub(super) fn write_private<T: Serialize>(
    path: &Path,
    value: &T,
    max_bytes: u64,
) -> io::Result<()> {
    verify_parent(path)?;
    match fs::symlink_metadata(path) {
        Ok(_) => {
            open_private(path, max_bytes)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() as u64 > max_bytes {
        return Err(invalid_input("Transcript metadata exceeds byte limit"));
    }
    private_file::write_atomic(path, &bytes)?;
    sync_parent(path)
}

pub(super) fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .ok_or_else(|| invalid_input("Missing transcript parent"))?;
        File::open(parent)?.sync_all()?;
    }
    #[cfg(windows)]
    let _ = path;
    Ok(())
}

pub(super) fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub(super) fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn refused(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests;
