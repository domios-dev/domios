use super::terminal::*;
use crate::sentinel::store::Sentinel;
use crate::sentinel::{
    machine::Route,
    source::TabBinding,
    store::{Owner, Store},
};
use codeterm_blueprint_types::blueprint_terminal::Config;
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Store, Sentinel) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_cron::clock::MockClock::starting_at(1_000),
    );
    let job = store
        .create(
            serde_json::from_value(json!({
                "name":"terminal-adapter", "observe":{"kind":"tab","id":"shell"},
                "terminal":{"pattern":null,"idle_ms":200}, "question":"reply?",
                "buckets":["reply"], "matches":{"reply":"(?s).*"},
                "routes":{"reply":"send:receiver"}, "timeout":"1h"
            }))
            .unwrap(),
            Owner {
                creator_tab_id: "creator".into(),
                team_id: Some("team".into()),
                workspace_id: "workspace".into(),
                cwd: ".".into(),
            },
            false,
        )
        .unwrap();
    (dir, store, job)
}

#[test]
fn blueprint_terminal_delta_settles_once_without_polling() {
    let (_dir, mut store, job) = fixture();
    assert!(store.due().unwrap().is_empty());
    let mut inbox = Inbox::default();
    inbox.feed(&job, Some(1), b"\x1b[32mfirst\x1b[0m\r\n", 1_000);
    inbox.feed(&job, Some(1), b"second", 1_100);
    assert_eq!(inbox.deadline(), Some(1_300));
    inbox.settle(1_299);
    assert!(inbox.take(&job.id).is_none());
    inbox.settle(1_300);
    assert_eq!(inbox.take(&job.id).as_deref(), Some("first\nsecond"));
    inbox.settle(9_000);
    assert!(inbox.take(&job.id).is_none());
    inbox.feed(&job, Some(1), b"third", 10_000);
    inbox.settle(10_200);
    assert_eq!(inbox.take(&job.id).as_deref(), Some("third"));
}

#[test]
fn blueprint_terminal_pattern_handles_split_ansi_and_utf8_with_bounded_delta() {
    let (_dir, _store, mut job) = fixture();
    job.set_terminal(Some(Config {
        pattern: Some("ready".into()),
        idle_ms: None,
    }));
    let mut inbox = Inbox::default();
    inbox.feed(&job, Some(1), b"\x1b[3", 1);
    inbox.feed(&job, Some(1), b"2mrea", 2);
    assert!(inbox.take(&job.id).is_none());
    inbox.feed(&job, Some(1), b"dy\x1b[0m", 3);
    assert_eq!(inbox.take(&job.id).as_deref(), Some("ready"));
    job.set_terminal(Some(Config {
        pattern: None,
        idle_ms: Some(20),
    }));
    let text = "\u{03bb}".repeat(MAX_PAYLOAD_BYTES);
    inbox.feed(&job, Some(1), text.as_bytes(), 4);
    inbox.settle(24);
    let delta = inbox.take(&job.id).unwrap();
    assert_eq!(delta.len(), MAX_PAYLOAD_BYTES);
    assert!(!delta.contains('\u{fffd}'));
}

#[test]
fn blueprint_terminal_spinner_timer_redraws_do_not_extend_semantic_idle() {
    let (_dir, _store, job) = fixture();
    let mut inbox = Inbox::default();
    inbox.feed(&job, Some(1), "\u{280b} working 1s".as_bytes(), 1_000);
    inbox.feed(&job, Some(1), "\r\u{2819} working 2s".as_bytes(), 1_100);
    assert_eq!(inbox.deadline(), Some(1_200));
    inbox.settle(1_200);
    assert_eq!(inbox.take(&job.id).as_deref(), Some(" working <time>"));
}

#[test]
fn blueprint_terminal_generation_change_discards_old_delta() {
    let (_dir, _store, job) = fixture();
    let mut inbox = Inbox::default();
    inbox.feed(&job, Some(1), b"old", 1_000);
    inbox.feed(&job, Some(2), b"new", 1_100);
    inbox.settle(1_300);
    assert_eq!(inbox.take(&job.id).as_deref(), Some("new"));
}

#[test]
fn blueprint_terminal_shell_scope_and_send_scope_are_team_bound() {
    let (_dir, _store, mut job) = fixture();
    let (mut mux, creator) = codeterm_pty::multiplexer::state::MultiplexerState::new();
    job.workspace_id = mux.find_workspace_for_tab(&creator).unwrap();
    let shell = mux.create_tab(&job.workspace_id).unwrap();
    mux.get_tab_mut(&shell).unwrap().team_id = job.team_id.clone();
    job.set_observe(Some(TabBinding::Tab(shell.clone())));
    assert!(crate::sentinel::source::validate_tab_scope(&mux, &job, &shell).is_ok());
    assert_eq!(
        crate::sentinel::routes::resolve_send(&mux, &job, &shell).unwrap(),
        Route::Send(shell.clone())
    );
    mux.get_tab_mut(&shell).unwrap().team_id = Some("foreign".into());
    assert!(crate::sentinel::source::validate_tab_scope(&mux, &job, &shell).is_err());
    assert!(crate::sentinel::routes::resolve_send(&mux, &job, &shell).is_err());
}

#[test]
fn blueprint_send_body_reaches_shell_and_agent_paths_verbatim_once() {
    let body = "quotes ' \" $() `ticks`\nsecond line \u{03bb}";
    for agent in [false, true] {
        let calls = std::cell::RefCell::new(Vec::new());
        crate::sentinel::routes::deliver_body_with(
            agent,
            body,
            |text| {
                calls.borrow_mut().push((false, text.to_string()));
                Ok(())
            },
            |text| {
                calls.borrow_mut().push((true, text.to_string()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*calls.borrow(), [(agent, body.to_string())]);
    }
}

#[test]
fn blueprint_text_input_queue_is_ordered_bounded_and_at_most_once() {
    let mut inbox = Inbox::default();
    assert!(inbox.enqueue("input", "".into()).is_err());
    assert!(inbox
        .enqueue("input", "x".repeat(MAX_PAYLOAD_BYTES + 1))
        .is_err());
    for index in 0..32 {
        inbox.enqueue("input", format!("message {index}")).unwrap();
    }
    assert!(inbox.enqueue("input", "overflow".into()).is_err());
    for index in 0..32 {
        assert_eq!(inbox.take("input"), Some(format!("message {index}")));
    }
    assert!(inbox.take("input").is_none());
}

#[test]
fn blueprint_text_input_admission_wakes_only_live_stage_and_routes_queued_body() {
    let (_dir, mut store, mut job) = fixture();
    job.set_terminal(None);
    job.set_observe(None);
    job.set_input(Some(codeterm_blueprint_types::sentinel_input::InputSpec {
        kind: codeterm_blueprint_types::sentinel_input::MediaKind::Text,
        data: codeterm_blueprint_types::sentinel_input::DataRef::Text("initial".into()),
        convert_to: None,
        convert_timeout_ms: None,
    }));
    store.jobs[0] = job.clone();
    let handle = crate::sentinel::scheduler::fixture_handle_without_app(store);
    let body = "\u{03bb} 'quotes' $() `ticks`\nnext";
    handle.enqueue_text(&job, body.into()).unwrap();
    let lease = handle.store.lock().due().unwrap().pop().unwrap();
    let received = handle.inbox.lock().take(&lease.job.id).unwrap();
    let calls = std::cell::RefCell::new(Vec::new());
    crate::sentinel::scheduler::advance(
        &handle.store,
        &lease,
        &crate::sentinel::probe::Output {
            success: true,
            bytes: received.as_bytes().to_vec(),
            text: received.clone(),
            ..Default::default()
        },
        || {
            crate::sentinel::provenance::Classification::regex(crate::sentinel::machine::Verdict {
                bucket: "reply".into(),
                confidence: 1.0,
            })
        },
        |job, transition, handler| handle.store.lock().claim_handler(job, transition, handler),
        |_, _, handler, _| {
            assert_eq!(handler.action, Route::Send("receiver".into()));
            crate::sentinel::routes::deliver_body_with(
                false,
                &received,
                |text| {
                    calls.borrow_mut().push(text.to_string());
                    Ok(())
                },
                |_| panic!("shell must use typed-input send"),
            )?;
            Ok(crate::sentinel::handlers::Status::Completed.into())
        },
        |_, _, _| Ok(()),
    );
    assert_eq!(*calls.borrow(), [body.to_string()]);
    handle.store.lock().release(&job.id);
    assert!(handle.store.lock().due().unwrap().is_empty());
    handle.store.lock().stop(&job.id, "test stop").unwrap();
    assert!(handle.enqueue_text(&job, "late".into()).is_err());
}
