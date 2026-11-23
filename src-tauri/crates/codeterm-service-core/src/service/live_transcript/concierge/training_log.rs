use std::{
    collections::{HashMap, VecDeque},
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

use codeterm_util::private_file;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::super::{TranscriptForward, TranscriptRow};
use super::{wall_now_ms, ServiceContext, TranscriptContent, TranscriptRange, TranscriptStore};

const SAMPLES: &str = "routing_samples.jsonl";
const FEEDBACK: &str = "routing_feedback.jsonl";
const MAX_LOG_BYTES: u64 = 50 * 1024 * 1024;
const MAX_UNREAD_ROWS: usize = 100;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Decision {
    pub rationale: String,
    pub action: String,
    pub target_tab_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Source {
    LocalConcierge,
    CloudConcierge,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RoutingSample {
    sample_id: String,
    ts: u64,
    source: Source,
    provider: String,
    model: Option<String>,
    #[serde(flatten)]
    input: RoutingInput,
    range: TranscriptRange,
    pub decision: Decision,
    pub forward_id: Option<String>,
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RoutingInput {
    owner_tab: OwnerTab,
    open_tabs: Vec<OpenTab>,
    context_rows: Vec<ContextRow>,
    pub unread_rows: Vec<UnreadRow>,
    forwarded_ranges: Vec<TranscriptRange>,
}

#[derive(Deserialize, Serialize)]
struct OwnerTab {
    id: String,
    title: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OpenTab {
    id: String,
    title: String,
    agent_type: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContextRow {
    seq: u64,
    dt_ms: i64,
    text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UnreadRow {
    seq: u64,
    at_ms: u64,
    pub text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Feedback {
    ts: u64,
    forward_id: String,
    range: TranscriptRange,
    verdict: String,
    note: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingLogSummary {
    log_path: std::path::PathBuf,
    feedback_path: std::path::PathBuf,
    total_samples: usize,
    send_count: usize,
    ignore_count: usize,
    wrong_send_count: usize,
    good_send_count: usize,
    recent_samples: Vec<RoutingSample>,
}

impl RoutingSample {
    pub(super) fn new(
        source: Source,
        provider: String,
        model: Option<String>,
        input: RoutingInput,
        range: TranscriptRange,
        decision: Decision,
    ) -> Self {
        Self {
            sample_id: uuid::Uuid::new_v4().to_string(),
            ts: wall_now_ms(),
            source,
            provider,
            model,
            input,
            range,
            decision,
            forward_id: None,
            latency_ms: None,
            system_prompt: None,
        }
    }
}

impl RoutingInput {
    pub(super) fn gather(
        svc: &ServiceContext,
        store: &TranscriptStore,
        owner: &str,
        range: TranscriptRange,
        acked_seq: u64,
        previous: &[TranscriptForward],
    ) -> io::Result<Self> {
        let unread_rows = unread(store, range)?;
        let first_at_ms = unread_rows.first().map_or(0, |row| row.at_ms);
        let context_rows = store
            .recent_context(acked_seq.min(range.from_seq.saturating_sub(1)))?
            .into_iter()
            .filter_map(speech)
            .map(|row| ContextRow {
                seq: row.seq,
                dt_ms: (i128::from(row.at_ms) - i128::from(first_at_ms))
                    .clamp(i128::from(i64::MIN), i128::from(i64::MAX))
                    as i64,
                text: row.text,
            })
            .collect();
        Ok(Self {
            owner_tab: OwnerTab {
                id: owner.into(),
                title: svc.mux.read().display_label_for_tab(owner),
            },
            open_tabs: visible_tabs(svc),
            context_rows,
            unread_rows,
            forwarded_ranges: previous.iter().map(|forward| forward.range).collect(),
        })
    }
}

fn speech(row: TranscriptRow) -> Option<UnreadRow> {
    match row.content {
        TranscriptContent::Speech { text } => Some(UnreadRow {
            seq: row.seq,
            at_ms: row.at_ms,
            text,
        }),
        _ => None,
    }
}

fn unread(store: &TranscriptStore, range: TranscriptRange) -> io::Result<Vec<UnreadRow>> {
    let mut rows = Vec::new();
    let mut from_seq = range.from_seq;
    while from_seq <= range.to_seq {
        let limit = (range.to_seq - from_seq + 1).min(MAX_UNREAD_ROWS as u64) as usize;
        let page = store.history(from_seq, limit)?;
        rows.extend(
            page.rows
                .into_iter()
                .take_while(|row| row.seq <= range.to_seq)
                .filter_map(speech),
        );
        if rows.len() > MAX_UNREAD_ROWS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Select at most 100 transcript speech rows",
            ));
        }
        let Some(next) = page.next_from_seq else {
            break;
        };
        from_seq = next;
    }
    Ok(rows)
}

pub(super) fn visible_tabs(svc: &ServiceContext) -> Vec<OpenTab> {
    let mux = svc.mux.read();
    let mut tabs: Vec<_> = mux
        .all_tabs()
        .filter(|tab| !tab.hidden && !tab.is_transcript_router())
        .map(|tab| OpenTab {
            id: tab.id.clone(),
            title: mux.display_label_for_tab(&tab.id),
            agent_type: tab.agent_type.clone(),
        })
        .collect();
    tabs.sort_by(|left, right| left.id.cmp(&right.id));
    tabs
}

pub(super) fn resolve_target(
    svc: &ServiceContext,
    owner: &str,
    requested: Option<&str>,
    tabs: &[OpenTab],
) -> (String, String) {
    let target = requested.and_then(|requested| {
        let requested = requested.trim();
        tabs.iter().find(|tab| tab.id == requested).or_else(|| {
            let requested = requested.to_lowercase();
            let mut titles = tabs
                .iter()
                .filter(|tab| tab.title.to_lowercase() == requested);
            titles.next().filter(|_| titles.next().is_none())
        })
    });
    target.map_or_else(
        || (owner.into(), svc.mux.read().display_label_for_tab(owner)),
        |tab| (tab.id.clone(), tab.title.clone()),
    )
}

// Call under the store's forward gate so JSONL records cannot interleave.
fn append(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    let (mut file, start, needs_newline) = match private_file::open_owner_only(path, MAX_LOG_BYTES)
    {
        Ok(mut reader) => {
            let start = reader.seek(SeekFrom::End(0))?;
            let needs_newline = if start > 0 {
                reader.seek(SeekFrom::End(-1))?;
                let mut byte = [0];
                reader.read_exact(&mut byte)?;
                byte[0] != b'\n'
            } else {
                false
            };
            (
                OpenOptions::new().append(true).open(path)?,
                start,
                needs_newline,
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            (private_file::create_new_owner_only(path)?, 0, false)
        }
        Err(error) => return Err(error),
    };
    if needs_newline {
        line.insert(0, b'\n');
    }
    if start + line.len() as u64 > MAX_LOG_BYTES {
        return Err(io::Error::other("Transcript training log is full"));
    }
    if let Err(error) = file.write_all(&line).and_then(|()| file.sync_data()) {
        file.set_len(start)?;
        file.sync_data()?;
        return Err(error);
    }
    Ok(())
}

pub(super) fn log(store: &TranscriptStore, sample: &RoutingSample) {
    let _gate = store.inner.forward_gate.lock();
    log_locked(store, sample);
}

pub(super) fn log_locked(store: &TranscriptStore, sample: &RoutingSample) {
    if let Err(error) = append(&store.inner.root.join(SAMPLES), sample) {
        log::debug!("[live-transcript] routing sample unavailable: {error}");
    }
}

impl ServiceContext {
    pub fn record_live_transcript_feedback(
        &self,
        owner: &str,
        forward_id: &str,
        verdict: &str,
        note: Option<String>,
    ) -> io::Result<TranscriptForward> {
        let store = self.open_live_transcript(owner).map_err(io::Error::other)?;
        let forward = store.update_forward_feedback(forward_id, verdict, |forward| {
            append(
                &store.inner.root.join(FEEDBACK),
                &Feedback {
                    ts: wall_now_ms(),
                    forward_id: forward_id.into(),
                    range: forward.range,
                    verdict: verdict.into(),
                    note,
                },
            )
        })?;
        self.emit_live_transcript_changed(owner);
        Ok(forward)
    }
}

impl TranscriptStore {
    pub fn training_log_summary(&self, limit: usize) -> io::Result<TrainingLogSummary> {
        let _gate = self.inner.forward_gate.lock();
        let log_path = self.inner.root.join(SAMPLES);
        let feedback_path = self.inner.root.join(FEEDBACK);
        let mut total_samples = 0;
        let mut send_count = 0;
        let mut ignore_count = 0;
        let mut recent = VecDeque::new();
        let limit = limit.min(200);
        visit::<RoutingSample>(&log_path, |sample| {
            total_samples += 1;
            match sample.decision.action.as_str() {
                "send" => send_count += 1,
                "ignore" => ignore_count += 1,
                _ => {}
            }
            if limit != 0 {
                if recent.len() == limit {
                    recent.pop_front();
                }
                recent.push_back(sample);
            }
        })?;
        let mut verdicts = HashMap::new();
        visit::<Feedback>(&feedback_path, |feedback| {
            verdicts.insert(feedback.forward_id, feedback.verdict);
        })?;
        Ok(TrainingLogSummary {
            log_path,
            feedback_path,
            total_samples,
            send_count,
            ignore_count,
            wrong_send_count: verdicts
                .values()
                .filter(|verdict| verdict.as_str() == "wrong_send")
                .count(),
            good_send_count: verdicts
                .values()
                .filter(|verdict| verdict.as_str() == "good_send")
                .count(),
            recent_samples: recent.into_iter().rev().collect(),
        })
    }
}

fn visit<T: DeserializeOwned>(path: &Path, mut on_row: impl FnMut(T)) -> io::Result<()> {
    let file: File = match private_file::open_owner_only(path, MAX_LOG_BYTES) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line)? != 0 {
        if let Ok(value) = serde_json::from_slice(&line) {
            on_row(value);
        }
        line.clear();
    }
    Ok(())
}
