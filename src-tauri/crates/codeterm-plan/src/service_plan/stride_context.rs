use crate::plan::Milestone;

pub const WORKER_STRIDE_LIMIT: usize = 6;
pub const WORKER_STRIDE_SUMMARY_MAX_CHARS: usize = 160;

// Pure bookkeeping (status_update) is noise — never surfaced to the worker.
const PINNED_KINDS: &[&str] = &["decision"];
const SIGNAL_KINDS: &[&str] = &["bounce", "note", "qa", "regression", "retrospective"];
const NOISE_KINDS: &[&str] = &["status_update"];

#[derive(Debug, Clone, Copy)]
pub struct WorkerStrideRef<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub text: &'a str,
    pub ts: u64,
}

impl<'a> From<&'a Milestone> for WorkerStrideRef<'a> {
    fn from(milestone: &'a Milestone) -> Self {
        Self {
            id: &milestone.id,
            kind: &milestone.kind,
            text: &milestone.text,
            ts: milestone.ts,
        }
    }
}

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

fn neutralize_stride_tags(text: String) -> String {
    text.replace("</codeterm-task-strides", "&lt;/codeterm-task-strides")
        .replace("<codeterm-task-strides", "&lt;codeterm-task-strides")
}

fn nth_char_byte_idx(text: &str, count: usize) -> usize {
    text.char_indices()
        .nth(count)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn summarize_stride_text(text: &str, max_chars: usize) -> (String, bool) {
    let collapsed = neutralize_stride_tags(one_line(text.trim()));
    if collapsed.chars().count() <= max_chars {
        return (collapsed, false);
    }
    let limit = max_chars.saturating_sub(1);
    let cut = nth_char_byte_idx(&collapsed, limit);
    let prefix = &collapsed[..cut];
    let min_word_cut = nth_char_byte_idx(prefix, (limit * 3) / 4);
    let boundary = prefix
        .rfind(char::is_whitespace)
        .filter(|&idx| idx >= min_word_cut)
        .unwrap_or(cut);
    let mut out = collapsed[..boundary].trim_end().to_string();
    out.push('…');
    (out, true)
}

fn format_stride_ts(ts: u64) -> String {
    if ts == 0 {
        return "-".to_string();
    }
    chrono::DateTime::from_timestamp_millis(ts as i64)
        .map(|dt| dt.format("%Y-%m-%d %H:%MZ").to_string())
        .unwrap_or_else(|| "-".to_string())
}

pub fn full_strides_command(plan_id: Option<&str>, task_id: Option<&str>) -> Option<String> {
    let task_id = task_id.map(str::trim).filter(|value| !value.is_empty())?;
    let plan_id = plan_id.map(str::trim).filter(|value| !value.is_empty());
    Some(match plan_id {
        Some(plan_id) => format!("domios plan task show {plan_id} {task_id}"),
        None => format!("domios task show {task_id}"),
    })
}

fn sanitize_attr_value(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|c| !matches!(c, '"' | '<' | '>' | '\n' | '\r'))
        .collect()
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

fn select_worker_stride_refs<'a>(strides: &[WorkerStrideRef<'a>]) -> Vec<WorkerStrideRef<'a>> {
    let mut selected: Vec<WorkerStrideRef<'a>> = strides
        .iter()
        .copied()
        .filter(|m| !is_noise(m.kind) && !m.text.trim().is_empty())
        .collect();
    selected.sort_by(|a, b| {
        tier_rank(a.kind)
            .cmp(&tier_rank(b.kind))
            .then(b.ts.cmp(&a.ts))
            .then(b.id.cmp(&a.id))
    });
    selected.truncate(WORKER_STRIDE_LIMIT);
    selected
}

pub fn render_worker_strides_from_refs(
    strides: &[WorkerStrideRef<'_>],
    plan_id: Option<&str>,
    task_id: Option<&str>,
    include_read_command: bool,
) -> Option<String> {
    let selected = select_worker_stride_refs(strides);
    if selected.is_empty() {
        return None;
    }
    let clean_task = task_id
        .map(sanitize_attr_value)
        .filter(|value| !value.is_empty());
    let mut out = match clean_task.as_deref() {
        Some(task_id) => format!(
            "<codeterm-task-strides task=\"{task_id}\" count=\"{}\">\n",
            selected.len()
        ),
        None => format!("<codeterm-task-strides count=\"{}\">\n", selected.len()),
    };
    out.push_str("Recent task strides (read-only digest, pinned/newest first):\n");
    for stride in selected {
        let ts = format_stride_ts(stride.ts);
        let (summary, truncated) =
            summarize_stride_text(stride.text, WORKER_STRIDE_SUMMARY_MAX_CHARS);
        if truncated && tier_rank(stride.kind) == 0 && !stride.id.trim().is_empty() {
            out.push_str(&format!(
                "- [{}] {} {} (full stride: {})\n",
                stride.kind,
                ts,
                summary,
                stride.id.trim()
            ));
        } else {
            out.push_str(&format!("- [{}] {} {}\n", stride.kind, ts, summary));
        }
    }
    if include_read_command {
        if let Some(command) = full_strides_command(plan_id, task_id) {
            out.push_str(&format!("Read full strides: `{command}`\n"));
        }
    }
    out.push_str("</codeterm-task-strides>\n");
    Some(out)
}

pub fn render_worker_strides(
    milestones: &[Milestone],
    plan_id: Option<&str>,
    task_id: Option<&str>,
) -> Option<String> {
    let refs: Vec<WorkerStrideRef<'_>> = milestones.iter().map(WorkerStrideRef::from).collect();
    render_worker_strides_from_refs(&refs, plan_id, task_id, true)
}

#[cfg(test)]
mod tests;
