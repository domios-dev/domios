//! Frame-motion decoder: classifies WHERE screen content moved between two
//! successive grid frames, independent of any provider. Row-hash offset voting
//! detects vertical scroll; rows leaving the viewport top become promotion
//! events (content entered history); per-row mutation streaks separate
//! streaming appends from in-place status churn (spinners, timers, counters).
//! Signal layer only — never delivery confirmation.

use serde::{Deserialize, Serialize};

/// Minimum rows agreeing on the same nonzero offset before a shift is accepted.
const MIN_SHIFT_VOTES: usize = 3;
/// A hash occurring more often than this in the previous frame is too ambiguous
/// to vote (blank separators, box-drawing borders).
const MAX_HASH_DUPES: usize = 2;
const MAX_SHIFT: i32 = 20;
/// Consecutive in-place mutations before a row is classed volatile.
const VOLATILE_STREAK: u8 = 3;
/// Quiet sample ticks before a row's mutation streak decays to zero.
const QUIET_RESET_TICKS: u8 = 40;
/// Changed-row fraction above which the frame is a repaint, not motion.
const REPAINT_FRACTION: f32 = 0.6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionKind {
    #[default]
    Still,
    /// Content scrolled up: rows were promoted into history.
    Scroll,
    /// New text accumulated in place (streaming tail growth or fresh rows).
    Stream,
    /// Every change landed on volatile rows — spinner/timer churn, no progress.
    StatusChurn,
    Repaint,
    Mixed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowClasses {
    pub shifted: Vec<u16>,
    pub appended: Vec<u16>,
    pub mutated: Vec<u16>,
    pub volatile: Vec<u16>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotionFrame {
    pub kind: MotionKind,
    /// Rows the viewport content moved up by (0 = no scroll).
    pub shift_up: u16,
    /// Text rows that left the viewport top this step — promoted into history.
    /// Immutable once emitted, safe to accumulate downstream.
    pub promoted: Vec<String>,
    pub classes: RowClasses,
    /// Rows with a live churn streak — the volatile region persists between
    /// spinner ticks and decays only after a sustained quiet run.
    pub volatile_region: Vec<u16>,
    /// Monotonic turn counter; bumps on every `begin_turn`.
    pub turn_seq: u32,
    /// Post-boundary rows frozen into the current turn this step (promoted off
    /// the viewport top). Immutable once emitted.
    pub turn_frozen: Vec<String>,
    /// Current-turn content still live on screen: post-boundary rows minus the
    /// volatile region and trailing composer/footer chrome.
    pub turn_live: Vec<String>,
}

struct PrevFrame {
    lines: Vec<String>,
    hashes: Vec<u64>,
    last_nonblank: Option<usize>,
}

#[derive(Default)]
pub struct MotionDecoder {
    prev: Option<PrevFrame>,
    mutation_streak: Vec<u8>,
    /// Sample ticks since the row last changed; long-quiet rows lose their streak.
    idle_ticks: Vec<u8>,
    /// Rows above this index belong to previous turns; `None` = turn tracking off.
    turn_boundary: Option<usize>,
    /// Hashes of pre-turn rows captured at `begin_turn` — survivors anchor the
    /// boundary when the TUI repaints or collapses instead of scrolling.
    turn_pre_hashes: std::collections::HashSet<u64>,
    turn_seq: u32,
}

impl MotionDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark everything currently on screen as pre-turn history. Call when an
    /// input submit is issued; subsequent appends/promotions belong to the turn.
    pub fn begin_turn(&mut self) {
        let (boundary, pre_hashes) = match self.prev.as_ref() {
            Some(prev) => (
                prev.last_nonblank.map_or(0, |ln| ln + 1),
                prev.lines
                    .iter()
                    .zip(&prev.hashes)
                    .filter(|(line, _)| !line.trim().is_empty() && !is_chrome_row(line))
                    .map(|(_, hash)| *hash)
                    .collect(),
            ),
            None => (0, std::collections::HashSet::new()),
        };
        self.turn_boundary = Some(boundary);
        self.turn_pre_hashes = pre_hashes;
        self.turn_seq += 1;
    }

    pub fn step(&mut self, lines: &[String]) -> MotionFrame {
        let hashes: Vec<u64> = lines.iter().map(|l| fnv64(l.trim_end().as_bytes())).collect();
        let last_nonblank = lines.iter().rposition(|l| !l.trim().is_empty());
        self.mutation_streak.resize(lines.len(), 0);
        self.idle_ticks.resize(lines.len(), 0);

        let Some(prev) = self.prev.take() else {
            self.prev = Some(PrevFrame {
                lines: lines.to_vec(),
                hashes,
                last_nonblank,
            });
            return MotionFrame {
                turn_seq: self.turn_seq,
                ..MotionFrame::default()
            };
        };

        let changed: Vec<usize> = (0..lines.len())
            .filter(|&r| prev.hashes.get(r) != Some(&hashes[r]))
            .collect();

        let shift_up = detect_shift_up(&prev, lines, &hashes);
        let mut classes = RowClasses::default();
        let mut promoted = Vec::new();

        let mut turn_frozen = Vec::new();
        if shift_up > 0 {
            let rotate = shift_up.min(self.mutation_streak.len());
            self.mutation_streak.rotate_left(rotate);
            self.idle_ticks.rotate_left(rotate);
            for streak in self.mutation_streak.iter_mut().rev().take(shift_up) {
                *streak = 0;
            }
            let exited = shift_up.min(prev.lines.len());
            promoted = prev.lines[..exited]
                .iter()
                .map(|l| l.trim_end().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if let Some(boundary) = self.turn_boundary {
                turn_frozen = prev.lines[boundary.min(exited)..exited]
                    .iter()
                    .map(|l| l.trim_end().to_string())
                    .filter(|l| !l.is_empty() && !is_chrome_row(l))
                    .collect();
                self.turn_boundary = Some(boundary.saturating_sub(shift_up));
            }
        }
        // Repaints and collapses replace pre-turn content without a scroll: the
        // boundary re-anchors to the deepest surviving pre-turn row each step.
        if let Some(boundary) = self.turn_boundary {
            if boundary > 0 {
                let survivor = (0..boundary.min(lines.len())).rev().find(|&r| {
                    !lines[r].trim().is_empty()
                        && !is_chrome_row(&lines[r])
                        && self.turn_pre_hashes.contains(&hashes[r])
                });
                self.turn_boundary = Some(survivor.map_or(0, |r| r + 1));
            }
        }

        for &r in &changed {
            let cur = lines[r].trim_end();
            let shifted_source = shift_up > 0 && prev.lines.get(r + shift_up).map(|p| p.trim_end()) == Some(cur);
            if shifted_source {
                classes.shifted.push(r as u16);
                self.mutation_streak[r] = 0;
                continue;
            }
            let prev_row = prev.lines.get(r).map(|l| l.trim_end()).unwrap_or("");
            let fresh_row = prev_row.is_empty()
                && !cur.is_empty()
                && prev.last_nonblank.is_none_or(|ln| r >= ln);
            let tail_growth = !prev_row.is_empty() && cur.len() > prev_row.len() && cur.starts_with(prev_row);
            self.idle_ticks[r] = 0;
            if fresh_row || tail_growth {
                classes.appended.push(r as u16);
                self.mutation_streak[r] = 0;
            } else {
                let streak = self.mutation_streak[r].saturating_add(1);
                self.mutation_streak[r] = streak;
                if streak >= VOLATILE_STREAK {
                    classes.volatile.push(r as u16);
                } else {
                    classes.mutated.push(r as u16);
                }
            }
        }
        // A streak survives quiet samples (spinners tick slower than we sample)
        // and decays only after a sustained quiet run.
        for r in 0..self.mutation_streak.len() {
            if !changed.contains(&r) {
                let idle = self.idle_ticks[r].saturating_add(1);
                self.idle_ticks[r] = idle;
                if idle >= QUIET_RESET_TICKS {
                    self.mutation_streak[r] = 0;
                }
            }
        }

        let rows = lines.len().max(1);
        let repaint = shift_up == 0 && changed.len() as f32 > REPAINT_FRACTION * rows as f32;
        let kind = if changed.is_empty() {
            MotionKind::Still
        } else if repaint {
            MotionKind::Repaint
        } else if shift_up > 0 {
            MotionKind::Scroll
        } else if !classes.appended.is_empty() {
            // Streaming with a live status footer still counts as streaming.
            MotionKind::Stream
        } else if !classes.volatile.is_empty() && classes.mutated.is_empty() {
            MotionKind::StatusChurn
        } else {
            MotionKind::Mixed
        };

        let volatile_region: Vec<u16> = self
            .mutation_streak
            .iter()
            .enumerate()
            .filter(|(_, &streak)| streak >= VOLATILE_STREAK)
            .map(|(r, _)| r as u16)
            .collect();
        let turn_live = self.turn_live_view(lines, last_nonblank, &volatile_region);

        self.prev = Some(PrevFrame {
            lines: lines.to_vec(),
            hashes,
            last_nonblank,
        });
        MotionFrame {
            kind,
            shift_up: shift_up as u16,
            promoted,
            classes,
            volatile_region,
            turn_seq: self.turn_seq,
            turn_frozen,
            turn_live,
        }
    }

    fn turn_live_view(
        &self,
        lines: &[String],
        last_nonblank: Option<usize>,
        volatile_region: &[u16],
    ) -> Vec<String> {
        let Some(boundary) = self.turn_boundary else {
            return Vec::new();
        };
        let Some(last) = last_nonblank else {
            return Vec::new();
        };
        if boundary > last {
            return Vec::new();
        }
        let mut view: Vec<String> = (boundary..=last)
            .filter(|r| !volatile_region.contains(&(*r as u16)))
            .map(|r| lines[r].trim_end().to_string())
            .collect();
        while view.last().is_some_and(|row| is_chrome_row(row)) {
            view.pop();
        }
        while view.first().is_some_and(|row| is_chrome_row(row)) {
            view.remove(0);
        }
        view
    }
}

/// Dominant upward offset by row-hash voting: each unambiguous current row that
/// matches a previous row at `prev_row = cur_row + k` votes for `k`.
fn detect_shift_up(prev: &PrevFrame, lines: &[String], hashes: &[u64]) -> usize {
    let mut prev_pos: std::collections::HashMap<u64, Vec<usize>> = std::collections::HashMap::new();
    for (r, (line, hash)) in prev.lines.iter().zip(&prev.hashes).enumerate() {
        if !line.trim().is_empty() {
            prev_pos.entry(*hash).or_default().push(r);
        }
    }
    let mut votes: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    for (r, (line, hash)) in lines.iter().zip(hashes).enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Some(positions) = prev_pos.get(hash) else {
            continue;
        };
        if positions.len() > MAX_HASH_DUPES {
            continue;
        }
        for &p in positions {
            let k = p as i32 - r as i32;
            if k.abs() <= MAX_SHIFT {
                *votes.entry(k).or_default() += 1;
            }
        }
    }
    let stable = votes.get(&0).copied().unwrap_or(0);
    let best_up = votes
        .iter()
        .filter(|(&k, _)| k > 0)
        .max_by_key(|(_, &count)| count);
    match best_up {
        Some((&k, &count)) if count >= MIN_SHIFT_VOTES && count > stable => k as usize,
        _ => 0,
    }
}

/// Composer/footer chrome trailing the turn content: blank rows, horizontal
/// rules, empty prompt glyph rows, and mode/interrupt hint footers.
fn is_chrome_row(row: &str) -> bool {
    let trimmed = row.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed
        .chars()
        .all(|c| matches!(c, '\u{2500}'..='\u{257f}' | '—' | '―'))
    {
        return true;
    }
    if matches!(trimmed.chars().next(), Some('❯' | '›' | '→')) && trimmed.chars().count() <= 2 {
        return true;
    }
    let lower = trimmed.to_lowercase();
    trimmed.starts_with('⏵')
        || trimmed.starts_with('▶')
        || lower.contains("esc to interrupt")
        || lower.contains("shift+tab to cycle")
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests;
