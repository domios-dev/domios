use super::*;

#[test]
fn draft_bounds_count_unicode_characters_and_description_bytes() {
    let mut draft = TabTodoDraft {
        title: "🦀".repeat(MAX_TAB_TODO_TITLE_CHARS),
        description: Some("я".repeat(MAX_TAB_TODO_DESCRIPTION_BYTES / 2)),
        link: None,
    };
    assert!(draft.validate().is_ok());
    draft.title.push('x');
    assert!(draft.validate().is_err());
    draft.title.pop();
    draft.description = Some("я".repeat(MAX_TAB_TODO_DESCRIPTION_BYTES / 2 + 1));
    assert!(draft.validate().is_err());
}

#[test]
fn external_link_requires_bounded_label_and_url() {
    let draft = TabTodoDraft {
        title: "Review source".into(),
        description: None,
        link: Some(TabTodoLinkDraft::Url {
            url: "https://example.test/issue/1".into(),
            label: "Issue".into(),
        }),
    };
    assert!(draft.validate().is_ok());
}

fn item(id: &str, position: u8, state: TabTodoState) -> TabTodoItem {
    TabTodoItem {
        id: id.into(),
        position,
        title: format!("Step {id}"),
        description: None,
        state,
        revision: 1,
        link: None,
        locked: true,
        reason: None,
        attempt: 0,
        counters: TabTodoCounters::default(),
        history: Vec::new(),
    }
}

fn event(kind: TabTodoEventKind, attempt: u32, at: u64) -> TabTodoItemEvent {
    TabTodoItemEvent {
        kind,
        attempt,
        actor_tab_id: None,
        reason: None,
        at,
    }
}

#[test]
fn attempt_spans_derive_each_attempt_duration_from_retained_history() {
    let mut restarted = item("a", 0, TabTodoState::InProgress);
    restarted.history = vec![
        event(TabTodoEventKind::Started, 1, 100),
        event(TabTodoEventKind::Blocked, 1, 110),
        event(TabTodoEventKind::Started, 1, 120),
        event(TabTodoEventKind::Completed, 1, 160),
        event(TabTodoEventKind::Reopened, 1, 200),
        event(TabTodoEventKind::Started, 2, 210),
    ];
    let spans = restarted.attempt_spans();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].started_at, 100);
    assert_eq!(spans[0].finished_at, Some(160));
    assert_eq!(spans[0].duration_secs, Some(60));
    assert_eq!(spans[1].attempt, 2);
    assert_eq!(spans[1].duration_secs, None);
}

#[test]
fn recitation_order_lists_blocked_first_and_remaining_excludes_terminal_items() {
    let checklist = TabTodoChecklist {
        proposal: None,
        tab_id: "tab".into(),
        generation: 1,
        started_at: 0,
        revision: 1,
        status: TabTodoChecklistStatus::Active,
        items: vec![
            item("open", 0, TabTodoState::Open),
            item("done", 1, TabTodoState::Done),
            item("current", 2, TabTodoState::InProgress),
            item("skipped", 3, TabTodoState::Skipped),
            item("blocked", 4, TabTodoState::Blocked),
        ],
    };
    let order: Vec<_> = checklist
        .recite_order()
        .into_iter()
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(order, ["blocked", "current", "open", "skipped", "done"]);
    let remaining: Vec<_> = checklist
        .remaining()
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(remaining, ["blocked", "current", "open"]);
}

#[test]
fn reasons_are_bounded_and_required_for_blocked_and_skipped() {
    assert!(TabTodoState::Blocked.requires_reason());
    assert!(TabTodoState::Skipped.requires_reason());
    assert!(!TabTodoState::InProgress.requires_reason());
    assert!(validate_tab_todo_reason("waiting on the parent").is_ok());
    assert!(validate_tab_todo_reason("  ").is_err());
    assert!(validate_tab_todo_reason(&"x".repeat(MAX_TAB_TODO_REASON_BYTES + 1)).is_err());
    assert!(tab_todo_bound_refusal().contains("propose a task split"));
}
