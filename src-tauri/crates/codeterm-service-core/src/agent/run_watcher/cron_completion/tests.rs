use super::*;
use std::io::Write;

fn answer(id: &str, content: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        kind: "assistant".into(),
        content: content.into(),
        ..Default::default()
    }
}

#[test]
fn cron_completion_persists_final_answer_before_close_in_both_event_orders() {
    for stop_first in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let transcript = directory.path().join("own-session.jsonl");
        let previous = answer("previous", "previous turn");
        std::fs::write(
            &transcript,
            format!("{}\n", serde_json::to_string(&previous).unwrap()),
        )
        .unwrap();
        let mut barrier = CompletionBarrier {
            stopped: stop_first,
            assistant_ids: HashSet::from([previous.id.clone()]),
        };
        let read = || -> Vec<ChatMsg> {
            std::fs::read_to_string(&transcript)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        };
        assert!(!barrier.ready(&read()));
        let final_answer = answer("final", "CRON_OK_TEST");
        let mut writer = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        writeln!(writer, "{}", serde_json::to_string(&final_answer).unwrap()).unwrap();
        writer.sync_all().unwrap();
        assert_eq!(barrier.ready(&read()), stop_first);
        barrier.stopped = true;
        let mut closed = false;
        if barrier.ready(&read()) {
            assert_eq!(read().last().unwrap().content, final_answer.content);
            closed = true;
        }
        assert!(closed);
    }
}

#[test]
fn cron_completion_cannot_close_for_tools_empty_answers_or_previous_turns() {
    let barrier = CompletionBarrier {
        stopped: true,
        assistant_ids: HashSet::from(["old".into()]),
    };
    assert!(!barrier.ready(&[answer("old", "previous answer")]));
    assert!(!barrier.ready(&[answer("new", " ")]));
    assert!(!barrier.ready(&[ChatMsg {
        kind: "tool_result".into(),
        content: "ok".into(),
        ..Default::default()
    }]));
    assert!(!barrier.ready(&[
        answer("new", "commentary"),
        ChatMsg {
            kind: "tool_use".into(),
            ..Default::default()
        }
    ]));
}

#[test]
fn cron_completion_cannot_attach_to_a_foreign_run() {
    let (mut mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&tab, "visible-pty".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut store =
        crate::cron::store::CronStore::new_with_path(fixture.temp.path().join("cron.json"));
    store.register_tab(&tab, "own-job", "own-run");
    let cron: crate::CronState = std::sync::Arc::new(parking_lot::Mutex::new(store));
    assert!(fixture.service.set_ext(cron).is_ok());
    let error = fixture
        .service
        .run_watcher
        .cron_completion_state(&fixture.service, &tab, "foreign-run")
        .err()
        .unwrap();
    assert_eq!(error, "cron run ownership changed");
    assert!(fixture.service.run_watcher.runs.lock().is_empty());
}
