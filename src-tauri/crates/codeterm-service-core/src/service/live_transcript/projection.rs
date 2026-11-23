use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io;

use super::retention::{read_summary, require_summary};
use super::store::{invalid_input, next_seq, visit_raw, StoreState};
use super::{
    AgentTranscriptPage, TranscriptBinding, TranscriptContent, TranscriptHistoryPage,
    TranscriptPosition, TranscriptRange, TranscriptRow, TranscriptSearchHit, TranscriptSearchPage,
    TranscriptSeq, TranscriptStore,
};

const FRAME_HEADER: &str = "AGENT TRANSCRIPT v1\n[CONTEXT]\n";
const UNREAD_HEADER: &str = "\n[UNREAD]\n";
const SEEN_HEADER: &str = "\n[SEEN]\n";
const LIVE_HEADER: &str = "\n[LIVE]\n";
const FRAME_CHARS: usize =
    FRAME_HEADER.len() + UNREAD_HEADER.len() + SEEN_HEADER.len() + LIVE_HEADER.len();
pub const MIN_AGENT_PAGE_CHARS: usize = FRAME_CHARS + 1;
pub const MAX_AGENT_PAGE_CHARS: usize = 64 * 1024;

const MAX_CONTEXT_ROWS: usize = 16;
const MAX_CONTEXT_CHARS: usize = 768;
const MAX_SCAN_SEGMENTS: usize = 4;
const MAX_HISTORY_ROWS: usize = 100;

impl TranscriptStore {
    pub fn recent_context(&self, through_seq: TranscriptSeq) -> io::Result<Vec<TranscriptRow>> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        self.context_locked(&state, through_seq)
    }

    fn context_locked(
        &self,
        state: &StoreState,
        through_seq: TranscriptSeq,
    ) -> io::Result<Vec<TranscriptRow>> {
        if through_seq > state.last_seq() {
            return Err(invalid_input("Context exceeds committed transcript"));
        }
        if through_seq == 0 {
            return Ok(Vec::new());
        }
        let last = state.segment_index(through_seq);
        let mut context = VecDeque::new();
        for segment in state.segments[..=last]
            .iter()
            .rev()
            .take(MAX_CONTEXT_ROWS)
            .filter(|segment| segment.has_raw())
        {
            let mut latest = VecDeque::new();
            visit_raw(&self.inner.root, segment, |row| {
                if row.seq > through_seq {
                    return Ok(false);
                }
                if !matches!(row.content, TranscriptContent::Speech { .. }) {
                    return Ok(true);
                }
                if latest.len() == MAX_CONTEXT_ROWS {
                    latest.pop_front();
                }
                latest.push_back(row);
                Ok(true)
            })?;
            while context.len() < MAX_CONTEXT_ROWS {
                let Some(row) = latest.pop_back() else {
                    break;
                };
                context.push_front(row);
            }
            if context.len() == MAX_CONTEXT_ROWS {
                break;
            }
        }
        Ok(context.into_iter().collect())
    }

    /// Format v1 counts Unicode scalar values, including framing and any transient LIVE text.
    /// Advance with next until absent and acknowledge only through_seq.
    pub fn read_page(
        &self,
        binding: &TranscriptBinding,
        position: Option<TranscriptPosition>,
        max_chars: usize,
        live_tail: Option<&str>,
    ) -> io::Result<AgentTranscriptPage> {
        if !(MIN_AGENT_PAGE_CHARS..=MAX_AGENT_PAGE_CHARS).contains(&max_chars) {
            return Err(invalid_input(
                "Transcript character bound cannot fit the frame or exceeds its maximum",
            ));
        }
        let state = self.inner.state.lock();
        let cursor = self.require_cursor(&state, binding)?;
        let committed_seq = state.last_seq();
        let first = next_seq(cursor.acked_seq)?;
        let end = next_seq(committed_seq)?;
        let mut position = position.unwrap_or(TranscriptPosition {
            seq: first,
            char_offset: 0,
        });
        if position.seq < first
            || position.seq > end
            || (position.seq == end && position.char_offset != 0)
        {
            return Err(invalid_input(
                "Transcript page position is outside the active cursor",
            ));
        }

        let mut output = AgentFrame {
            remaining: max_chars - FRAME_CHARS,
            ..AgentFrame::default()
        };
        if position.seq == first && position.char_offset == 0 && output.remaining >= 8 {
            let context = self.context_locked(&state, cursor.acked_seq)?;
            let per_row = (output.remaining / 4).min(MAX_CONTEXT_CHARS) / context.len().max(1);
            if per_row >= 2 {
                for row in context {
                    let line = format_row(&row);
                    output.remaining -= push_context(&mut output.context, &line, per_row);
                }
            }
        }
        let mut gaps = Vec::new();
        for segment in state
            .segments
            .iter()
            .skip(state.segment_index(position.seq))
            .take(MAX_SCAN_SEGMENTS)
        {
            if output.remaining == 0 {
                break;
            }
            if !segment.has_raw() {
                let gap = TranscriptRange {
                    from_seq: position.seq,
                    to_seq: segment.range.to_seq,
                };
                let marker = format!("[raw unavailable {}–{}]\n", gap.from_seq, gap.to_seq);
                let count = marker.chars().count();
                let (marker, count) = if output.remaining >= count {
                    (marker.as_str(), count)
                } else {
                    ("…", 1)
                };
                output.section(gap.to_seq, cursor.seen_seq).push_str(marker);
                output.remaining -= count;
                gaps.push(gap);
                position = TranscriptPosition {
                    seq: next_seq(gap.to_seq)?,
                    char_offset: 0,
                };
                continue;
            }
            visit_raw(&self.inner.root, segment, |row| {
                if row.seq < position.seq {
                    return Ok(true);
                }
                if row.seq != position.seq {
                    return Err(super::store::invalid_data(
                        "Transcript page encountered a sequence gap",
                    ));
                }
                let line = format_row(&row);
                let line_chars = line.chars().count();
                if position.char_offset >= line_chars {
                    return Err(invalid_input(
                        "Transcript page character offset exceeds row",
                    ));
                }
                let written = output.remaining.min(line_chars - position.char_offset);
                output
                    .section(row.seq, cursor.seen_seq)
                    .extend(line.chars().skip(position.char_offset).take(written));
                output.remaining -= written;
                position.char_offset += written;
                if position.char_offset == line_chars {
                    position = TranscriptPosition {
                        seq: next_seq(row.seq)?,
                        char_offset: 0,
                    };
                }
                Ok(output.remaining != 0)
            })?;
        }
        let next = (position.seq < end).then_some(position);
        if next.is_none() {
            if let Some(live_tail) = live_tail {
                push_live(&mut output.live, live_tail, output.remaining);
            }
        }
        Ok(AgentTranscriptPage {
            text: output.render(),
            through_seq: position.seq - 1,
            committed_seq,
            next,
            gaps,
        })
    }

    pub fn history(
        &self,
        from_seq: TranscriptSeq,
        scan_rows: usize,
    ) -> io::Result<TranscriptHistoryPage> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        let window = self.scan_window(&state, from_seq, scan_rows)?;
        Ok(TranscriptHistoryPage {
            rows: window.rows,
            gaps: window.gaps,
            next_from_seq: window.next_from_seq,
        })
    }

    pub fn search(
        &self,
        query: &str,
        from_seq: TranscriptSeq,
        scan_rows: usize,
    ) -> io::Result<TranscriptSearchPage> {
        if query.is_empty() || query.len() > 1024 {
            return Err(invalid_input("Empty or oversized transcript search"));
        }
        let state = self.inner.state.lock();
        state.require_healthy()?;
        let window = self.scan_window(&state, from_seq, scan_rows)?;
        let mut hits = Vec::new();
        for row in window.rows {
            if content_text(&row.content).is_some_and(|text| text.contains(query)) {
                hits.push(TranscriptSearchHit::Raw { row });
            }
        }
        for range in window.summarized {
            let summary = require_summary(&self.inner.root, range)?;
            if summary.text.contains(query) {
                hits.push(TranscriptSearchHit::Summary { summary });
            }
        }
        hits.sort_by_key(|hit| match hit {
            TranscriptSearchHit::Raw { row } => row.seq,
            TranscriptSearchHit::Summary { summary } => summary.range.to_seq,
        });
        Ok(TranscriptSearchPage {
            hits,
            gaps: window.gaps,
            next_from_seq: window.next_from_seq,
        })
    }

    pub fn summary(&self, range: TranscriptRange) -> io::Result<Option<super::SegmentSummary>> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        let index = state.segment_index(range.from_seq);
        if state
            .segments
            .get(index)
            .is_none_or(|segment| segment.range != range)
            || !state.is_sealed(index)
        {
            return Err(invalid_input("Exact sealed transcript range required"));
        }
        if state.segments[index].has_raw() {
            read_summary(&self.inner.root, range)
        } else {
            require_summary(&self.inner.root, range).map(Some)
        }
    }

    fn scan_window(
        &self,
        state: &StoreState,
        from_seq: TranscriptSeq,
        scan_rows: usize,
    ) -> io::Result<Window> {
        if from_seq == 0 || !(1..=MAX_HISTORY_ROWS).contains(&scan_rows) {
            return Err(invalid_input("Invalid transcript page bounds"));
        }
        let mut window = Window::default();
        let mut next = from_seq;
        for segment in state
            .segments
            .iter()
            .skip(state.segment_index(from_seq))
            .take(MAX_SCAN_SEGMENTS)
        {
            if window.rows.len() == scan_rows {
                break;
            }
            if segment.has_raw() {
                visit_raw(&self.inner.root, segment, |row| {
                    if row.seq < next {
                        return Ok(true);
                    }
                    if row.seq != next {
                        return Err(super::store::invalid_data(
                            "Transcript history encountered a sequence gap",
                        ));
                    }
                    next = next_seq(row.seq)?;
                    window.rows.push(row);
                    Ok(window.rows.len() < scan_rows)
                })?;
            } else {
                window.gaps.push(TranscriptRange {
                    from_seq: next,
                    to_seq: segment.range.to_seq,
                });
                next = next_seq(segment.range.to_seq)?;
            }
            if segment.summarized && next > segment.range.to_seq {
                window.summarized.push(segment.range);
            }
        }
        window.next_from_seq = (next <= state.last_seq()).then_some(next);
        Ok(window)
    }
}

#[derive(Default)]
struct Window {
    rows: Vec<TranscriptRow>,
    gaps: Vec<TranscriptRange>,
    summarized: Vec<TranscriptRange>,
    next_from_seq: Option<TranscriptSeq>,
}

#[derive(Default)]
struct AgentFrame {
    context: String,
    unread: String,
    seen: String,
    live: String,
    remaining: usize,
}

impl AgentFrame {
    fn section(&mut self, seq: TranscriptSeq, seen_seq: TranscriptSeq) -> &mut String {
        if seq <= seen_seq {
            &mut self.seen
        } else {
            &mut self.unread
        }
    }

    fn render(self) -> String {
        format!(
            "{FRAME_HEADER}{}{UNREAD_HEADER}{}{SEEN_HEADER}{}{LIVE_HEADER}{}",
            self.context, self.unread, self.seen, self.live
        )
    }
}

fn content_text(content: &TranscriptContent) -> Option<&str> {
    match content {
        TranscriptContent::Speech { text } => Some(text),
        TranscriptContent::Paused { reason } => Some(reason),
        TranscriptContent::Resumed => None,
    }
}

fn format_row(row: &TranscriptRow) -> String {
    let mut line = format!("{} | ", row.seq);
    match &row.content {
        TranscriptContent::Speech { text } => push_live(&mut line, text, usize::MAX),
        TranscriptContent::Paused { reason } => {
            line.push_str("PAUSED ");
            push_live(&mut line, reason, usize::MAX);
        }
        TranscriptContent::Resumed => line.push_str("RESUMED"),
    }
    line.push('\n');
    line
}

fn push_context(output: &mut String, line: &str, max_chars: usize) -> usize {
    let count = line.chars().count();
    if count <= max_chars {
        output.push_str(line);
        return count;
    }
    output.extend(line.chars().take(max_chars - 2));
    output.push_str("…\n");
    max_chars
}

fn push_live(output: &mut String, text: &str, mut remaining: usize) {
    for ch in text.chars() {
        if remaining == 0 {
            break;
        }
        if ch.is_control() || ch == '\\' {
            let escaped = ch.escape_default();
            let count = escaped.len();
            if count > remaining {
                break;
            }
            let _ = write!(output, "{escaped}");
            remaining -= count;
        } else {
            output.push(ch);
            remaining -= 1;
        }
    }
}
