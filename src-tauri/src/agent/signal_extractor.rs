use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Half-open row band `[top, bottom)`. Geometry-relative, used for both the
/// region model and the changed-row bounding box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowRange {
    pub top: u16,
    pub bottom: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeltaMode {
    #[default]
    None,
    Append,
    InPlace,
    FullRepaint,
}

/// Mechanical motion verdict — NOT a lifecycle state. The plugin combines this
/// with its own busy fingerprint to reach `turn_complete` vs `working_streaming`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Motion {
    #[default]
    Still,
    Active,
    Streaming,
    Animating,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridView {
    pub lines: Vec<String>,
    pub last_nonblank_row: Option<u16>,
    pub screen_hash: [u8; 32],
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenDelta {
    pub changed_rows: Vec<u16>,
    pub appended_text: Option<String>,
    pub mode: DeltaMode,
    pub bytes_since_last: u64,
    pub churn: Option<RowRange>,
}

/// Two-clock quiescence — the no-false-positive core. `byte_idle` is reset by
/// any byte; `semantic_idle` is reset only by a change OUTSIDE the volatile region.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quiescence {
    pub byte_idle_ms: u64,
    pub semantic_idle_ms: u64,
    pub cursor_idle_ms: u64,
    pub volatile_only: bool,
    pub kind: Motion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorSignal {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
    pub on_last_nonblank_row: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionSignal {
    pub content: RowRange,
    pub volatile: RowRange,
    pub content_text: String,
}

/// The input contract for `classify_pane_state` — a provider-neutral structured
/// view of the PTY stream, produced per output step from a vt100 grid diff.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamSignals {
    pub cols: u16,
    pub rows: u16,
    pub screen: GridView,
    pub tail: String,
    pub delta: ScreenDelta,
    pub quiescence: Quiescence,
    pub cursor: CursorSignal,
    pub regions: RegionSignal,
}

/// Plugin-declared volatile region hint. Default = none (every change is
/// semantic). `bottom_rows` is the spinner/status band the semantic clock ignores.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolatileHint {
    pub bottom_rows: u16,
}

struct PrevGrid {
    row_hashes: Vec<u64>,
    last_nonblank: Option<u16>,
    cursor: (u16, u16),
}

/// Core, provider-agnostic engine: turns the raw PTY byte stream into
/// `StreamSignals` via a vt100 grid diff against the previous step. Pure w.r.t.
/// time — `now_ms` is injected on every step for deterministic fixture replay.
pub struct SignalExtractor {
    parser: vt100::Parser,
    tail_rows: usize,
    volatile: VolatileHint,
    prev: Option<PrevGrid>,
    last_byte_ms: Option<u64>,
    last_semantic_ms: Option<u64>,
    last_cursor_ms: Option<u64>,
}

const DEFAULT_TAIL_ROWS: usize = 60;
const SCROLLBACK_ROWS: usize = 500;

impl SignalExtractor {
    pub fn new(rows: u16, cols: u16, volatile: VolatileHint) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, SCROLLBACK_ROWS),
            tail_rows: DEFAULT_TAIL_ROWS,
            volatile,
            prev: None,
            last_byte_ms: None,
            last_semantic_ms: None,
            last_cursor_ms: None,
        }
    }

    /// Resize to real pane geometry. Reflows the grid and drops the prev-grid so
    /// the expected repaint is not mistaken for a content delta, while the
    /// semantic clock is preserved (a resize is not progress).
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
        self.prev = None;
    }

    pub fn step(&mut self, chunk: &[u8], now_ms: u64) -> StreamSignals {
        if !chunk.is_empty() {
            self.parser.process(chunk);
            self.last_byte_ms = Some(now_ms);
        }

        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let volatile_top = rows.saturating_sub(self.volatile.bottom_rows);

        let mut lines: Vec<String> = Vec::with_capacity(rows as usize);
        let mut row_hashes: Vec<u64> = Vec::with_capacity(rows as usize);
        let mut last_nonblank: Option<u16> = None;
        for row in 0..rows {
            let text = screen.contents_between(row, 0, row, cols);
            let trimmed = text.trim_end().to_string();
            if !trimmed.is_empty() {
                last_nonblank = Some(row);
            }
            row_hashes.push(fxhash64(trimmed.as_bytes()));
            lines.push(trimmed);
        }

        let mut hasher = Sha256::new();
        for line in &lines {
            let t = line.trim();
            if !t.is_empty() {
                hasher.update(t.as_bytes());
            }
        }
        let screen_hash: [u8; 32] = hasher.finalize().into();

        let changed_rows: Vec<u16> = match &self.prev {
            Some(prev) => (0..rows)
                .filter(|&r| {
                    prev.row_hashes
                        .get(r as usize)
                        .is_none_or(|&h| h != row_hashes[r as usize])
                })
                .collect(),
            None => Vec::new(),
        };

        let churn = changed_rows.first().map(|&top| RowRange {
            top,
            bottom: changed_rows.last().unwrap() + 1,
        });

        // Append == the change is confined to the tail (prior last-nonblank row and
        // below): new rows, or an extension of the line being typed. A change to an
        // EARLIER row is an in-place overwrite, not growth.
        let appended = match self.prev.as_ref().map(|p| p.last_nonblank) {
            Some(Some(ln)) => !changed_rows.is_empty() && changed_rows.iter().all(|&r| r >= ln),
            Some(None) => !changed_rows.is_empty(),
            None => false,
        };

        let in_volatile = |r: u16| self.volatile.bottom_rows > 0 && r >= volatile_top;
        let volatile_only =
            !changed_rows.is_empty() && changed_rows.iter().all(|&r| in_volatile(r));
        let semantic_change = changed_rows.iter().any(|&r| !in_volatile(r));

        let mode = if changed_rows.is_empty() {
            DeltaMode::None
        } else if appended {
            DeltaMode::Append
        } else if volatile_only {
            DeltaMode::InPlace
        } else if changed_rows.len() as f32 > 0.6 * rows as f32 {
            DeltaMode::FullRepaint
        } else {
            DeltaMode::InPlace
        };

        let appended_text = if mode == DeltaMode::Append {
            Some(
                changed_rows
                    .iter()
                    .map(|&r| lines[r as usize].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        } else {
            None
        };

        if semantic_change {
            self.last_semantic_ms = Some(now_ms);
        } else if self.last_semantic_ms.is_none() && last_nonblank.is_some() {
            self.last_semantic_ms = Some(now_ms);
        }

        let (crow, ccol) = screen.cursor_position();
        let cursor_moved = match &self.prev {
            Some(prev) => prev.cursor != (crow, ccol),
            None => true,
        };
        if cursor_moved {
            self.last_cursor_ms = Some(now_ms);
        }

        let idle = |since: Option<u64>| since.map_or(0, |t| now_ms.saturating_sub(t));
        let byte_idle_ms = idle(self.last_byte_ms);
        let semantic_idle_ms = idle(self.last_semantic_ms);
        let cursor_idle_ms = idle(self.last_cursor_ms);

        let kind = if byte_idle_ms > 0 {
            Motion::Still
        } else if semantic_change {
            if mode == DeltaMode::Append {
                Motion::Streaming
            } else {
                Motion::Active
            }
        } else if volatile_only {
            Motion::Animating
        } else {
            Motion::Active
        };

        let content = RowRange {
            top: 0,
            bottom: volatile_top,
        };
        let volatile = RowRange {
            top: volatile_top,
            bottom: rows,
        };
        let content_text = lines[..volatile_top as usize].join("\n");

        let cursor = CursorSignal {
            row: crow,
            col: ccol,
            visible: !screen.hide_cursor(),
            on_last_nonblank_row: last_nonblank == Some(crow),
        };

        let tail = self.tail_text(&lines, last_nonblank);

        self.prev = Some(PrevGrid {
            row_hashes,
            last_nonblank,
            cursor: (crow, ccol),
        });

        StreamSignals {
            cols,
            rows,
            screen: GridView {
                lines,
                last_nonblank_row: last_nonblank,
                screen_hash,
            },
            tail,
            delta: ScreenDelta {
                changed_rows,
                appended_text,
                mode,
                bytes_since_last: chunk.len() as u64,
                churn,
            },
            quiescence: Quiescence {
                byte_idle_ms,
                semantic_idle_ms,
                cursor_idle_ms,
                volatile_only,
                kind,
            },
            cursor,
            regions: RegionSignal {
                content,
                volatile,
                content_text,
            },
        }
    }

    fn tail_text(&self, lines: &[String], last_nonblank: Option<u16>) -> String {
        let end = last_nonblank.map_or(0, |r| r as usize + 1);
        let start = end.saturating_sub(self.tail_rows);
        lines[start..end].join("\n")
    }
}

/// Cheap, stable per-row content hash (FNV-1a 64) for grid diffing. Stable
/// across runs (no random seed) so fixture replay is deterministic.
fn fxhash64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests;
