use super::*;
use codeterm_types::agent::{TabTodoChecklistStatus, TabTodoCounters};

fn write_item(json: Value) -> AgentTodoWriteItem {
    serde_json::from_value(json).expect("todo write item")
}

fn item(id: &str, state: TabTodoState) -> TabTodoItem {
    TabTodoItem {
        id: id.into(),
        position: 0,
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

#[test]
fn todo_write_demotes_before_promoting_the_in_progress_item() {
    let change = |id: &str, state| StateChange {
        item_id: id.into(),
        state,
        reason: None,
    };
    let mut changes = vec![
        change("new-active", TabTodoState::InProgress),
        change("completed", TabTodoState::Done),
        change("old-active", TabTodoState::Open),
    ];
    prioritize_state_changes(&mut changes);
    let order: Vec<_> = changes
        .iter()
        .map(|change| change.item_id.as_str())
        .collect();
    assert_eq!(order, ["completed", "old-active", "new-active"]);
}

#[test]
fn todo_write_accepts_blocked_and_skipped_only_with_a_reason() {
    let blocked = write_item(json!({
        "content": "Gate",
        "status": "blocked",
        "activeForm": "Waiting on the gate",
        "reason": "waiting on the canonical gate",
    }));
    assert!(validate_write_item(&blocked).is_ok());
    assert_eq!(blocked.status.state(), TabTodoState::Blocked);
    let skipped = write_item(json!({
        "content": "Docs",
        "status": "skipped",
        "activeForm": "Skipping docs",
    }));
    assert!(validate_write_item(&skipped)
        .unwrap_err()
        .contains("needs a reason"));
    let restart = write_item(json!({
        "content": "Build",
        "status": "in_progress",
        "activeForm": "Rebuilding",
        "reason": "QA rejected the build",
    }));
    assert!(validate_write_item(&restart).is_ok());
}

#[test]
fn completion_reply_lists_the_remaining_open_items() {
    let checklist = TabTodoChecklist {
        tab_id: "tab".into(),
        generation: 1,
        started_at: 0,
        revision: 3,
        status: TabTodoChecklistStatus::Active,
        items: vec![
            item("done", TabTodoState::Done),
            item("next", TabTodoState::Open),
            item("blocked", TabTodoState::Blocked),
            item("skipped", TabTodoState::Skipped),
        ],
    };
    let reply = with_remaining(checklist, None);
    let remaining: Vec<_> = reply["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(remaining, ["blocked", "next"]);
}

#[test]
fn progress_line_shows_attempts_and_derivable_durations() {
    let mut restarted = item("build", TabTodoState::InProgress);
    restarted.attempt = 2;
    restarted.counters.started = 2;
    restarted.counters.completed = 1;
    restarted.history = vec![
        codeterm_types::agent::TabTodoItemEvent {
            kind: codeterm_types::agent::TabTodoEventKind::Started,
            attempt: 1,
            actor_tab_id: None,
            reason: None,
            at: 10,
        },
        codeterm_types::agent::TabTodoItemEvent {
            kind: codeterm_types::agent::TabTodoEventKind::Completed,
            attempt: 1,
            actor_tab_id: None,
            reason: None,
            at: 40,
        },
    ];
    let line = render_progress(&restarted);
    assert!(line.starts_with("attempt 2 (started 2, completed 1"));
    assert!(line.ends_with("durations #1 30s"));
}
