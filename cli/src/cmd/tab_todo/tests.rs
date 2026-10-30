use super::*;
use codeterm_types::agent::{TabTodoChecklistStatus, TabTodoCounters};

fn write_item(json: Value) -> AgentTodoWriteItem {
    serde_json::from_value(json).expect("todo write item")
}

#[test]
fn ease_todo_schema_reports_all_fields_and_defaults_active_form() {
    let error = parse_write_items(json!([
        {"title":"Wrong", "state":"open", "status":"bad", "activeForm":7},
        {"content":"Second", "status":false, "link":42}
    ])).err().unwrap();
    for field in ["title", "state", "status", "activeForm", "link", "content", "Accepted schema"] {
        assert!(error.contains(field), "{error}");
    }
    let items = parse_write_items(json!([{ "content":"Survey", "status":"pending" }])).unwrap();
    let entries = write_entries(items).unwrap();
    assert_eq!(entries[0].content, "Survey");
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

struct FakeRpc {
    calls: Vec<(String, Option<Value>)>,
    reply: Value,
}

impl RpcClient for FakeRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, crate::client::SocketCallError> {
        self.calls.push((method.to_string(), params));
        Ok(self.reply.clone())
    }
}

fn written(items: Vec<TabTodoItem>) -> Value {
    json!({
        "checklist": TabTodoChecklist {
        proposal: None,
            tab_id: "orchestrator".into(),
            generation: 1,
            started_at: 0,
            revision: 2,
            status: TabTodoChecklistStatus::Active,
            items,
        },
        "omitted_locked_items": 1,
    })
}

#[test]
fn todo_write_declares_the_whole_list_in_one_call_without_a_prior_checklist() {
    let mut rpc = FakeRpc {
        calls: Vec::new(),
        reply: written(vec![
            item("seeded", TabTodoState::Open),
            item("mine", TabTodoState::InProgress),
        ]),
    };
    let reply = write_declared_todos(
        &mut rpc,
        "orchestrator",
        vec![
            write_item(json!({ "content": "Survey", "status": "completed", "activeForm": "Surveying" })),
            write_item(json!({ "content": "Merge", "status": "in_progress", "activeForm": "Merging", "link": "https://example.test/pr/1" })),
        ],
    )
    .unwrap();

    assert_eq!(rpc.calls.len(), 1, "no checklist read precedes the write");
    let (method, params) = &rpc.calls[0];
    assert_eq!(method, "tab.todo.write");
    assert_eq!(
        params.as_ref().unwrap(),
        &json!({
            "tab_id": "orchestrator",
            "caller_tab_id": "orchestrator",
            "items": [
                { "content": "Survey", "state": "done" },
                { "content": "Merge", "state": "in_progress", "link": "https://example.test/pr/1" },
            ],
        })
    );
    assert_eq!(reply["omitted_locked_items"], 1);
    assert_eq!(reply["remaining"].as_array().unwrap().len(), 2);
    assert!(reply["message"]
        .as_str()
        .unwrap()
        .contains("1 omitted locked item"));
}

#[test]
fn todo_write_refuses_bad_entries_before_calling_the_daemon() {
    let mut rpc = FakeRpc {
        calls: Vec::new(),
        reply: Value::Null,
    };
    let skipped = write_item(
        json!({ "content": "Docs", "status": "skipped", "activeForm": "Skipping docs" }),
    );
    let error = write_declared_todos(&mut rpc, "tab", vec![skipped]).unwrap_err();
    assert!(error.contains("needs a reason"), "{error}");
    let blank_form =
        write_item(json!({ "content": "Build", "status": "pending", "activeForm": " " }));
    assert!(write_declared_todos(&mut rpc, "tab", vec![blank_form]).is_err());
    let blocked = write_item(json!({
        "content": "Gate",
        "status": "blocked",
        "activeForm": "Waiting on the gate",
        "reason": "waiting on the canonical gate",
    }));
    assert_eq!(write_entry(blocked).unwrap().state, TabTodoState::Blocked);
    assert!(rpc.calls.is_empty());
}

#[test]
fn completion_reply_lists_the_remaining_open_items() {
    let checklist = TabTodoChecklist {
        proposal: None,
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

#[test]
fn todo_write_forwards_id_only_status_updates() {
    let mut rpc = FakeRpc {
        calls: Vec::new(),
        reply: written(vec![item("seeded", TabTodoState::Done)]),
    };
    write_declared_todos(
        &mut rpc,
        "orchestrator",
        vec![write_item(json!({
            "id": "seeded", "status": "completed"
        }))],
    )
    .unwrap();
    assert_eq!(
        rpc.calls[0].1.as_ref().unwrap()["items"][0],
        json!({
            "id": "seeded", "content": "", "state": "done"
        })
    );
}
