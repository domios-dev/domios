use super::*;
use std::sync::mpsc::channel;

#[test]
fn second_command_waits_for_post_model_idle_event() {
    let signal = Arc::new(ScreenChangeSignal::default());
    let idle = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (tx, rx) = channel();
    let worker_signal = signal.clone();
    let worker_idle = idle.clone();
    let worker = std::thread::spawn(move || {
        let writes = std::cell::RefCell::new(Vec::new());
        execute_command(
            || true,
            || {
                writes.borrow_mut().push("model");
                Ok(())
            },
            || true,
        )
        .unwrap();
        let mut first = true;
        wait_for_idle(
            &worker_signal,
            Instant::now() + Duration::from_secs(2),
            || {
                let ready = worker_idle.load(std::sync::atomic::Ordering::Acquire);
                if first {
                    first = false;
                    tx.send(writes.borrow().clone()).unwrap();
                }
                post_command_idle(false, ready)
            },
        )
        .unwrap();
        execute_command(
            || worker_idle.load(std::sync::atomic::Ordering::Acquire),
            || {
                writes.borrow_mut().push("effort");
                Ok(())
            },
            || true,
        )
        .unwrap();
        writes.into_inner()
    });
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), ["model"]);
    idle.store(true, std::sync::atomic::Ordering::Release);
    signal.mark_changed();
    assert_eq!(worker.join().unwrap(), ["model", "effort"]);
}

#[test]
fn idle_before_wait_and_idle_between_observation_and_wait_are_retained() {
    let signal = ScreenChangeSignal::default();
    wait_for_idle(&signal, Instant::now() + Duration::from_secs(2), || {
        Ok(true)
    })
    .unwrap();
    let mut observations = 0;
    wait_for_idle(&signal, Instant::now() + Duration::from_secs(2), || {
        observations += 1;
        if observations == 1 {
            signal.mark_changed();
            post_command_idle(false, false)
        } else {
            Ok(true)
        }
    })
    .unwrap();
    assert_eq!(observations, 2);
}

#[test]
fn foreign_turn_refuses_second_command_even_if_idle_arrives() {
    let signal = ScreenChangeSignal::default();
    let mut observations = 0;
    let result = wait_for_idle(&signal, Instant::now() + Duration::from_secs(2), || {
        observations += 1;
        if observations == 1 {
            signal.mark_changed();
            Ok(false)
        } else {
            post_command_idle(true, true)
        }
    });
    assert!(result.unwrap_err().contains("foreign user turn"));
    assert!(post_command_idle(true, false).is_err());
}

#[test]
fn deadline_and_closed_event_stream_do_not_claim_idle() {
    let signal = ScreenChangeSignal::default();
    assert!(wait_for_idle(&signal, Instant::now(), || Ok(true))
        .unwrap_err()
        .contains("deadline"));
    let mut screens = crate::pty::screen_state::ScreenState::new();
    let signal = screens.subscribe_screen_changes("closed-command-pty");
    screens.remove("closed-command-pty");
    assert!(
        wait_for_idle(&signal, Instant::now() + Duration::from_secs(2), || Ok(
            false
        ))
        .unwrap_err()
        .contains("closed")
    );
}

fn declared_transcript() -> ModelSwitchTranscript {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../../../../plugins/provider-claude/plugin.json"
    ))
    .unwrap();
    serde_json::from_value(manifest["commands"]["modelSwitchCommands"]["transcript"].clone())
        .unwrap()
}

fn command_records() -> String {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/tests/fixtures/model-switch/command-transcript.json"
    )))
    .unwrap();
    cases
        .iter()
        .filter(|case| case["foreign"] == false)
        .map(|case| case["record"].to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn captured_model_and_effort_records_need_no_idle_delay_when_idle_is_observed() {
    let records: Vec<serde_json::Value> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/tests/fixtures/model-switch/live-command-records.json"
    )))
    .unwrap();
    let bytes = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let signal = ScreenChangeSignal::default();
    let generation = signal.generation();
    let mut observations = 0;
    wait_for_idle(&signal, Instant::now() + Duration::from_secs(1), || {
        observations += 1;
        post_command_idle(declared_transcript().has_foreign_user_turn(&bytes)?, true)
    })
    .unwrap();
    assert_eq!(observations, 1);
    assert_eq!(signal.generation(), generation);
    assert_eq!(
        run_fixture_sequence(&bytes, &declared_transcript()).unwrap(),
        ["/model", "/effort high"]
    );
}

fn run_fixture_sequence(
    bytes: &str,
    source: &ModelSwitchTranscript,
) -> Result<Vec<&'static str>, String> {
    let writes = std::cell::RefCell::new(Vec::new());
    execute_command(
        || true,
        || {
            writes.borrow_mut().push("/model");
            Ok(())
        },
        || true,
    )?;
    wait_for_idle(
        &ScreenChangeSignal::default(),
        Instant::now() + Duration::from_secs(2),
        || {
            let foreign = source.has_foreign_user_turn(bytes)?;
            post_command_idle(foreign, true)
        },
    )?;
    execute_command(
        || true,
        || {
            writes.borrow_mut().push("/effort high");
            Ok(())
        },
        || true,
    )?;
    Ok(writes.into_inner())
}

#[test]
fn declared_local_command_records_allow_model_then_target_effort() {
    assert_eq!(
        run_fixture_sequence(&command_records(), &declared_transcript()).unwrap(),
        ["/model", "/effort high"]
    );
}

#[test]
fn typed_prompt_between_local_command_records_refuses_target_effort() {
    let bytes = format!(
        "{}\n{}",
        command_records(),
        serde_json::json!({"type":"user","message":{"content":"Please inspect the build"}})
    );
    assert!(run_fixture_sequence(&bytes, &declared_transcript())
        .unwrap_err()
        .contains("foreign user turn"));
}

#[test]
fn command_wrappers_do_not_hide_extra_prompt_text_or_attachments() {
    let source = declared_transcript();
    for content in [
        serde_json::json!("<local-command-stdout>output</local-command-stdout>\nPlease inspect the build"),
        serde_json::json!("<local-command-stdout>output</local-command-stdout>\nPlease inspect the build\n<local-command-stdout>more</local-command-stdout>"),
        serde_json::json!([{"type":"text","text":"<local-command-stdout>output</local-command-stdout>"}, {"type":"text","text":"Please inspect the build"}]),
        serde_json::json!([{"type":"image","source":{}}]),
        serde_json::json!("<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args>other-model</command-args>"),
    ] {
        let bytes = serde_json::json!({"type":"user","message":{"content":content}}).to_string();
        assert!(source.has_foreign_user_turn(&bytes).unwrap());
    }
}

#[test]
fn command_exclusions_follow_an_opaque_provider_declaration() {
    let source = ModelSwitchTranscript {
        record_type: "prompt-record".into(),
        content_path: vec!["body".into()],
        prefix: "[output]".into(),
        suffix: "[/output]".into(),
        ignored_content_regexes: vec!["^opaque-caveat$".into()],
    };
    for content in ["[output]done[/output]", "opaque-caveat"] {
        let bytes = serde_json::json!({"type":"prompt-record","body":content}).to_string();
        assert!(!source.has_foreign_user_turn(&bytes).unwrap());
    }
    let bytes = serde_json::json!({"type":"prompt-record","body":"real prompt"}).to_string();
    assert!(source.has_foreign_user_turn(&bytes).unwrap());
}
