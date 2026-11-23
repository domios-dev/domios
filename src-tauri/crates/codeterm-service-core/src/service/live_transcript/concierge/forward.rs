use super::super::{BindingTarget, TranscriptForward, TranscriptRow};
use super::{wall_now_ms, ServiceContext, TranscriptContent, TranscriptRange, TranscriptStore};

const MAX_BACKTRACK_ROWS: usize = 16;
const MAX_SPEECH_GAP_MS: u64 = 8_000;
const MAX_SENT_CHARS: usize = 4_000;
const MAX_EXCERPT_CHARS: usize = 240;

impl ServiceContext {
    pub fn record_live_transcript_forward(
        &self,
        router_tab_id: &str,
        target_tab_id: &str,
        body: &str,
    ) {
        let (owner, target_title) = {
            let mux = self.mux.read();
            let Some(owner) = mux
                .get_tab(router_tab_id)
                .and_then(|tab| tab.transcript_router_owner())
            else {
                return;
            };
            (owner.to_owned(), mux.display_label_for_tab(target_tab_id))
        };
        let result = self
            .open_live_transcript(&owner)
            .and_then(|store| record(&store, target_tab_id, target_title, body));
        match result {
            Ok(true) => self.emit_live_transcript_changed(&owner),
            Ok(false) => {}
            Err(error) => {
                log::warn!("[live-transcript] owner={owner} forward receipt unavailable: {error}")
            }
        }
    }
}

fn record(
    store: &TranscriptStore,
    target_tab_id: &str,
    target_title: String,
    body: &str,
) -> Result<bool, String> {
    let body = body.trim();
    if body.is_empty() {
        return Ok(false);
    }
    let _gate = store.inner.forward_gate.lock();
    let binding = store.binding();
    let state = store.routing_state()?;
    let committed = store.committed_seq();
    let to_seq = if state.attempted_through > 0 {
        state.attempted_through.min(committed)
    } else {
        committed
    };
    if to_seq == 0 {
        return Ok(false);
    }
    let cursor = match &binding.target {
        BindingTarget::Detached => store.attached_cursor(),
        _ => store.cursor(&binding),
    }
    .map_err(|error| error.to_string())?;
    let epoch_id = match binding.target {
        BindingTarget::Attached { epoch_id } => Some(epoch_id),
        _ => None,
    };
    let from_seq = cursor.acked_seq.saturating_add(1).min(to_seq);
    let search_from = from_seq.saturating_sub(MAX_BACKTRACK_ROWS as u64).max(1);
    let page = store
        .history(search_from, (from_seq - search_from + 1) as usize)
        .map_err(|error| error.to_string())?;
    let previous = store.forwards().map_err(|error| error.to_string())?;
    let range = forward_range(from_seq, to_seq, &page.rows, &previous);

    let sent_text: String = body.chars().take(MAX_SENT_CHARS).collect();
    let mut chars = sent_text.chars();
    let mut sent_excerpt: String = chars.by_ref().take(MAX_EXCERPT_CHARS).collect();
    if chars.next().is_some() {
        sent_excerpt.push('…');
    }
    store
        .record_forward(TranscriptForward {
            forward_id: uuid::Uuid::new_v4().to_string(),
            epoch_id,
            range,
            at_ms: wall_now_ms(),
            target_tab_id: target_tab_id.to_owned(),
            target_title,
            sent_text,
            sent_excerpt,
            rationale: None,
        })
        .map_err(|error| error.to_string())?;
    Ok(true)
}

pub(super) fn forward_range(
    mut from_seq: u64,
    to_seq: u64,
    rows: &[TranscriptRow],
    previous: &[TranscriptForward],
) -> TranscriptRange {
    if let Some(forward) = previous
        .iter()
        .rev()
        .find(|forward| forward.range.from_seq <= to_seq && to_seq <= forward.range.to_seq)
    {
        return forward.range;
    }
    let forwarded_through = previous
        .iter()
        .map(|forward| forward.range.to_seq)
        .filter(|&seq| seq < to_seq)
        .max()
        .unwrap_or(0);
    from_seq = from_seq.max(forwarded_through + 1);
    let start_seq = from_seq;
    let mut speech = rows.iter().rev().skip_while(move |row| row.seq > start_seq);
    if let Some(start) = speech.next().filter(|row| {
        row.seq == from_seq && matches!(row.content, TranscriptContent::Speech { .. })
    }) {
        let mut next_at_ms = start.at_ms;
        for row in speech.take(MAX_BACKTRACK_ROWS) {
            if row.seq <= forwarded_through
                || row.seq + 1 != from_seq
                || !matches!(row.content, TranscriptContent::Speech { .. })
                || next_at_ms.abs_diff(row.at_ms) > MAX_SPEECH_GAP_MS
            {
                break;
            }
            from_seq = row.seq;
            next_at_ms = row.at_ms;
        }
    }
    TranscriptRange { from_seq, to_seq }
}
