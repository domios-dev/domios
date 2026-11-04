use crate::plan::Milestone;

pub const WORKER_STRIDE_LIMIT: usize = 6;

// Pure bookkeeping (status_update) is noise — never surfaced to the worker.
const PINNED_KINDS: &[&str] = &["decision"];
const SIGNAL_KINDS: &[&str] = &["bounce", "note", "qa", "regression", "retrospective"];
const NOISE_KINDS: &[&str] = &["status_update"];

pub(super) fn is_noise(kind: &str) -> bool {
    NOISE_KINDS.contains(&kind)
}

pub(super) fn tier_rank(kind: &str) -> u8 {
    if PINNED_KINDS.contains(&kind) {
        0
    } else if SIGNAL_KINDS.contains(&kind) {
        1
    } else {
        2
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// Across kinds, prioritizing pinned/signal; newest-first within a tier, stable
// via id tiebreak. Empty-text and pure-bookkeeping strides are dropped.
pub fn select_worker_strides(milestones: &[Milestone]) -> Vec<&Milestone> {
    let mut selected: Vec<&Milestone> = milestones
        .iter()
        .filter(|m| !is_noise(&m.kind) && !m.text.trim().is_empty())
        .collect();
    selected.sort_by(|a, b| {
        tier_rank(&a.kind)
            .cmp(&tier_rank(&b.kind))
            .then(b.ts.cmp(&a.ts))
            .then(b.id.cmp(&a.id))
    });
    selected.truncate(WORKER_STRIDE_LIMIT);
    selected
}

pub fn render_worker_strides(milestones: &[Milestone]) -> Option<String> {
    let selected = select_worker_strides(milestones);
    if selected.is_empty() {
        return None;
    }
    let mut out = String::from("Recent task strides (read-only context, pinned/newest first):\n");
    for m in selected {
        out.push_str(&format!("- [{}] {}\n", m.kind, one_line(m.text.trim())));
    }
    Some(out)
}

#[cfg(test)]
mod tests;
