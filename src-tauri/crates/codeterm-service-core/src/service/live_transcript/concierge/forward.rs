use super::super::{
    BindingTarget, TranscriptBinding, TranscriptCursor, TranscriptForward, TranscriptRow,
};
use super::training_log::{self, Decision, RoutingInput, RoutingSample, Source};
use super::Dispatch;
use super::{wall_now_ms, ServiceContext, TranscriptContent, TranscriptRange, TranscriptStore};

const MAX_BACKTRACK_ROWS: usize = 16;
const MAX_SPEECH_GAP_MS: u64 = 8_000;
const MAX_SENT_CHARS: usize = 4_000;
const MAX_EXCERPT_CHARS: usize = 240;

impl ServiceContext {
    pub fn ack_transcript_and_log(
        &self,
        store: &TranscriptStore,
        binding: &TranscriptBinding,
        owner: &str,
        through: u64,
    ) -> std::io::Result<TranscriptCursor> {
        let BindingTarget::Attached { epoch_id } = &binding.target else {
            return store.ack(binding, through);
        };
        let _gate = store.inner.forward_gate.lock();
        let (before, cursor) = store.ack_with_previous(binding, through)?;
        if cursor.acked_seq > before {
            let range = TranscriptRange {
                from_seq: before + 1,
                to_seq: cursor.acked_seq,
            };
            let _ = (|| {
                let previous = store.forwards()?;
                if previous.iter().any(|forward| {
                    forward.range.from_seq <= range.to_seq && forward.range.to_seq >= range.from_seq
                }) {
                    return Ok::<(), std::io::Error>(());
                }
                if let Some(history) = store.epoch(epoch_id)? {
                    let input = RoutingInput::gather(self, store, owner, range, before, &previous)?;
                    training_log::log_locked(
                        store,
                        &RoutingSample::new(
                            Source::CloudConcierge,
                            history.epoch.provider,
                            history.epoch.model,
                            input,
                            range,
                            Decision {
                                action: "ignore".into(),
                                target_tab_id: None,
                                message: None,
                                rationale:
                                    "Concierge acknowledged the range without forwarding it.".into(),
                            },
                        ),
                    );
                }
                Ok(())
            })();
        }
        Ok(cursor)
    }

    pub fn live_transcript_cursor_changed(&self, owner: &str, ack: bool) {
        if ack {
            if let Some(entry) = self
                .ext_or_init(Dispatch::default)
                .owners
                .lock()
                .get_mut(owner)
            {
                entry.last_route_error = None;
            }
        }
        self.emit_live_transcript_changed(owner);
    }

    pub fn record_live_transcript_forward(
        &self,
        router_tab_id: &str,
        target_tab_id: &str,
        body: &str,
    ) {
        let (owner, provider, model, target_title) = {
            let mux = self.mux.read();
            let Some(router) = mux.get_tab(router_tab_id) else {
                return;
            };
            let Some(owner) = router.transcript_router_owner() else {
                return;
            };
            let provider = router.agent_type.clone().unwrap_or_default();
            let model = router
                .agent_runtime_config
                .as_ref()
                .and_then(|config| config["model"].as_str())
                .map(str::to_owned);
            (
                owner.to_owned(),
                provider,
                model,
                mux.display_label_for_tab(target_tab_id),
            )
        };
        let result = self.open_live_transcript(&owner).and_then(|store| {
            record(
                self,
                &store,
                &owner,
                (provider, model),
                target_tab_id,
                target_title,
                body,
            )
        });
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
    svc: &ServiceContext,
    store: &TranscriptStore,
    owner: &str,
    provider_model: (String, Option<String>),
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
    if binding.target == BindingTarget::Local {
        return Ok(false);
    }
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
    let rows = backtrack_rows(store, from_seq)?;
    let previous = store.forwards().map_err(|error| error.to_string())?;
    let range = forward_range(from_seq, to_seq, &rows, &previous);
    let epoch = epoch_id
        .as_deref()
        .and_then(|id| store.epoch(id).ok().flatten());
    let (provider, model) = epoch.map_or(provider_model, |history| {
        (history.epoch.provider, history.epoch.model)
    });
    let forward = receipt(epoch_id, range, target_tab_id, target_title, body, None);
    let decision = Decision {
        action: "send".into(),
        target_tab_id: Some(forward.target_tab_id.clone()),
        message: Some(forward.sent_text.clone()),
        rationale: "Concierge forwarded an addressed transcript request.".into(),
    };
    let forward_id = forward.forward_id.clone();
    store
        .record_forward(forward)
        .map_err(|error| error.to_string())?;
    let unread_range = TranscriptRange { from_seq, to_seq };
    if let Ok(input) =
        RoutingInput::gather(svc, store, owner, unread_range, cursor.acked_seq, &previous)
    {
        let mut sample = RoutingSample::new(
            Source::CloudConcierge,
            provider,
            model,
            input,
            range,
            decision,
        );
        sample.forward_id = Some(forward_id);
        training_log::log_locked(store, &sample);
    }
    Ok(true)
}

pub(super) fn backtrack_rows(
    store: &TranscriptStore,
    from_seq: u64,
) -> Result<Vec<TranscriptRow>, String> {
    let search_from = from_seq.saturating_sub(MAX_BACKTRACK_ROWS as u64).max(1);
    store
        .history(search_from, (from_seq - search_from + 1) as usize)
        .map(|page| page.rows)
        .map_err(|error| error.to_string())
}

pub(super) fn receipt(
    epoch_id: Option<String>,
    range: TranscriptRange,
    target_tab_id: &str,
    target_title: String,
    body: &str,
    rationale: Option<String>,
) -> TranscriptForward {
    let sent_text: String = body.chars().take(MAX_SENT_CHARS).collect();
    let mut chars = sent_text.chars();
    let mut sent_excerpt: String = chars.by_ref().take(MAX_EXCERPT_CHARS).collect();
    if chars.next().is_some() {
        sent_excerpt.push('…');
    }
    TranscriptForward {
        forward_id: uuid::Uuid::new_v4().to_string(),
        epoch_id,
        range,
        at_ms: wall_now_ms(),
        target_tab_id: target_tab_id.to_owned(),
        target_title,
        sent_text,
        sent_excerpt,
        rationale,
        feedback: None,
    }
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
