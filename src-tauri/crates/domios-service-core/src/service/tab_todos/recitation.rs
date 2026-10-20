use codeterm_types::agent::{
    TabTodoChecklist, TabTodoRecitation, TabTodoRecitedItem, TabTodoState, TabTodoSummary,
    MAX_TAB_TODO_ITEMS, MAX_TAB_TODO_SUMMARY_BYTES, MAX_TAB_TODO_SUMMARY_TITLES,
};

const MAX_STEP_LINE_TITLE_CHARS: usize = 80;

pub(super) fn summary(checklist: &TabTodoChecklist) -> TabTodoSummary {
    let ordered = checklist.recite_order();
    let unfinished: Vec<_> = ordered
        .iter()
        .filter(|item| !item.state.is_terminal())
        .collect();
    let mut titles = Vec::new();
    let mut used = 0usize;
    for item in unfinished.iter().take(MAX_TAB_TODO_SUMMARY_TITLES) {
        let cost = item.title.len() + usize::from(!titles.is_empty());
        if used + cost > MAX_TAB_TODO_SUMMARY_BYTES {
            break;
        }
        used += cost;
        titles.push(item.title.clone());
    }
    let count = |state: TabTodoState| {
        checklist
            .items
            .iter()
            .filter(|item| item.state == state)
            .count()
            .min(MAX_TAB_TODO_ITEMS) as u8
    };
    TabTodoSummary {
        tab_id: checklist.tab_id.clone(),
        generation: checklist.generation,
        unfinished_count: unfinished.len().min(MAX_TAB_TODO_ITEMS) as u8,
        blocked_count: count(TabTodoState::Blocked),
        skipped_count: count(TabTodoState::Skipped),
        titles,
    }
}

pub(super) fn recitation_text(recitation: &TabTodoRecitation) -> String {
    let counts = Counts::of(&recitation.items);
    let mut text = format!(
        "Checklist: {} open, {} blocked, {} skipped",
        counts.open, counts.blocked, counts.skipped
    );
    for item in recitation
        .items
        .iter()
        .filter(|item| !item.state.is_terminal())
        .take(MAX_TAB_TODO_SUMMARY_TITLES)
    {
        let mut line = format!("\n- [{}] {}", state_label(item.state), item.title);
        if let Some(reason) = &item.reason {
            line.push_str(": ");
            line.push_str(reason);
        }
        let remaining = MAX_TAB_TODO_SUMMARY_BYTES.saturating_sub(text.len());
        if remaining == 0 {
            break;
        }
        text.push_str(bounded_utf8_prefix(&line, remaining));
    }
    text
}

/// Compact heartbeat step line; `None` when the list is empty or complete.
pub(crate) fn step_line(recitation: &TabTodoRecitation) -> Option<String> {
    let counts = Counts::of(&recitation.items);
    if counts.open + counts.blocked == 0 {
        return None;
    }
    let first = |state: TabTodoState| {
        recitation
            .items
            .iter()
            .find(|item| item.state == state)
            .map(|item| format!("\"{}\"", bounded_chars(&item.title)))
            .unwrap_or_else(|| "none".into())
    };
    Some(format!(
        "Checklist step: current {}; next {}; {} open, {} blocked",
        first(TabTodoState::InProgress),
        first(TabTodoState::Open),
        counts.open,
        counts.blocked
    ))
}

struct Counts {
    open: usize,
    blocked: usize,
    skipped: usize,
}

impl Counts {
    fn of(items: &[TabTodoRecitedItem]) -> Self {
        let count = |wanted: &[TabTodoState]| {
            items
                .iter()
                .filter(|item| wanted.contains(&item.state))
                .count()
        };
        Self {
            open: count(&[TabTodoState::Open, TabTodoState::InProgress]),
            blocked: count(&[TabTodoState::Blocked]),
            skipped: count(&[TabTodoState::Skipped]),
        }
    }
}

fn state_label(state: TabTodoState) -> &'static str {
    match state {
        TabTodoState::Open => "pending",
        TabTodoState::InProgress => "in_progress",
        TabTodoState::Done => "completed",
        TabTodoState::Blocked => "blocked",
        TabTodoState::Skipped => "skipped",
    }
}

fn bounded_chars(title: &str) -> String {
    let mut chars = title.chars();
    let mut bounded: String = chars.by_ref().take(MAX_STEP_LINE_TITLE_CHARS).collect();
    if chars.next().is_some() {
        bounded.push('…');
    }
    bounded
}

pub(super) fn bounded_utf8_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
