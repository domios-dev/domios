use super::*;
use crate::agent_runtime::RuntimeReduction;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Spec {
    schema_version: u32,
    provenance: Value,
    context: RuntimeContext,
    ages: Vec<usize>,
    description_chars: usize,
    report_chars: Vec<usize>,
    suffix_text_chars: usize,
    repetitions: usize,
    suffix: Value,
}

fn note(context: &RuntimeContext, method: &str, mut params: Value) -> RuntimeFrame {
    if params.get("threadId").is_none() {
        params["threadId"] = json!(context.thread_id);
    }
    RuntimeFrame::Notification {
        method: method.into(),
        params,
    }
}

fn spawn(
    context: &RuntimeContext,
    spec: &Spec,
    index: usize,
    report: usize,
    completed: bool,
) -> RuntimeFrame {
    let child = format!("opaque-child-{index:02}");
    let description = format!("{child}:");
    let description = format!(
        "{description}{}",
        "d".repeat(spec.description_chars - description.len())
    );
    let mut agents = json!({});
    agents[&child] = if completed {
        json!({"status":"completed","message":"r".repeat(report)})
    } else {
        json!({"status":"pendingInit"})
    };
    note(
        context,
        if completed {
            "item/completed"
        } else {
            "item/started"
        },
        json!({
            "turnId":context.active_turn_id,
            "item":{"id":format!("opaque-spawn-{index:02}"),"type":"collabAgentToolCall","tool":"spawnAgent",
                "status":if completed {"completed"} else {"inProgress"},"senderThreadId":context.thread_id,
                "receiverThreadIds":[child],"prompt":description,"agentsStates":agents}
        }),
    )
}

fn suffix(value: &Value, size: usize) -> Value {
    match value {
        Value::String(text) if text == "$suffix" => json!("x".repeat(size)),
        Value::Array(values) => {
            Value::Array(values.iter().map(|value| suffix(value, size)).collect())
        }
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), suffix(value, size)))
                .collect(),
        ),
        value => value.clone(),
    }
}

async fn step(
    provider: &JsProvider,
    context: &RuntimeContext,
    state: &Value,
    frame: RuntimeFrame,
) -> (RuntimeReduction, u64) {
    let before = state.clone();
    assert!(serde_json::to_vec(&frame).unwrap().len() <= MAX_RUNTIME_VALUE_BYTES);
    let started = Instant::now();
    let result = provider
        .runtime_reduce(context, state, &[frame])
        .await
        .unwrap_or_else(|error| panic!("child retention native hook refused: {error:?}"));
    let elapsed = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    assert_eq!(state, &before, "native fixture input remains reusable");
    (result, elapsed)
}

fn emitted(events: &[AgentRuntimeEvent], id: &str) -> Value {
    for event in events.iter().rev() {
        if let AgentRuntimeEvent::ChatItem { item, .. } = event {
            if item.id == id {
                return serde_json::to_value(item).unwrap();
            }
        }
    }
    panic!("missing emitted child-retention row {id}");
}

async fn background_guard(
    provider: &JsProvider,
    spec: &Spec,
    context: &RuntimeContext,
    checkpoint: &Value,
) {
    let mut guarded = context.clone();
    let root = context.thread_id.as_ref().unwrap();
    let old_turn = context.active_turn_id.as_ref().unwrap();
    let bg = "opaque-child-99";
    let bg_turn = "opaque-background-turn";
    guarded
        .thread_parents
        .push(serde_json::from_value(json!({"threadId":bg,"parentThreadId":root})).unwrap());
    guarded.active_turns.push(
        serde_json::from_value(json!({"threadId":bg,"turnId":bg_turn,"parentThreadId":root}))
            .unwrap(),
    );
    let (first, _) = step(
        provider,
        &guarded,
        checkpoint,
        spawn(&guarded, spec, 99, 0, false),
    )
    .await;
    let (working, _) = step(
        provider,
        &guarded,
        &first.state,
        note(
            &guarded,
            "turn/started",
            json!({"threadId":bg,"turn":{"id":bg_turn}}),
        ),
    )
    .await;
    let (settled, _) = step(
        provider,
        &guarded,
        &working.state,
        note(
            &guarded,
            "turn/completed",
            json!({"turn":{"id":old_turn,"status":"completed"}}),
        ),
    )
    .await;
    guarded.active_turn_id = Some("opaque-next-root".into());
    guarded.active_turns = serde_json::from_value(json!([
        {"threadId":root,"turnId":"opaque-next-root"}, {"threadId":bg,"turnId":bg_turn,"parentThreadId":root}
    ])).unwrap();
    let (next, _) = step(
        provider,
        &guarded,
        &settled.state,
        note(
            &guarded,
            "turn/started",
            json!({"turn":{"id":"opaque-next-root"}}),
        ),
    )
    .await;
    let (child_done, _) = step(
        provider,
        &guarded,
        &next.state,
        note(
            &guarded,
            "turn/completed",
            json!({"threadId":bg,"turn":{"id":bg_turn,"status":"completed"}}),
        ),
    )
    .await;
    assert_eq!(child_done.events.len(), 1);
    let event = serde_json::to_value(&child_done.events[0]).unwrap();
    assert_eq!(event["kind"], "turnState");
    assert_eq!(event["threadId"], bg);
    assert_eq!(event["turnId"], bg_turn);
    assert_eq!(event["state"], "idle");
    assert_eq!(event["parentThreadId"], json!(root));
    assert_eq!(child_done.state["activeTurnId"], "opaque-next-root");
    let (duplicate, _) = step(
        provider,
        &guarded,
        &child_done.state,
        note(
            &guarded,
            "turn/completed",
            json!({"threadId":bg,"turn":{"id":bg_turn,"status":"completed"}}),
        ),
    )
    .await;
    assert!(
        duplicate.events.is_empty(),
        "duplicate child completion settles no second card"
    );
    guarded.active_turns =
        serde_json::from_value(json!([{"threadId":root,"turnId":"opaque-next-root"}])).unwrap();
    let (late, _) = step(
        provider,
        &guarded,
        &child_done.state,
        note(
            &guarded,
            "turn/completed",
            json!({"turn":{"id":old_turn,"status":"completed"}}),
        ),
    )
    .await;
    assert!(late.events.is_empty());
    assert_eq!(late.state["activeTurnId"], "opaque-next-root");
    let (foreign, _) = step(
        provider,
        &guarded,
        &child_done.state,
        note(
            &guarded,
            "turn/started",
            json!({"threadId":"opaque-foreign","turn":{"id":"foreign-turn"}}),
        ),
    )
    .await;
    assert!(foreign.events.is_empty());
    for changed_epoch in [false, true] {
        let mut replacement = guarded.clone();
        if changed_epoch {
            replacement.epoch += 1;
        } else {
            replacement.binding_nonce = "opaque-other-nonce".into();
        }
        let reset = provider
            .runtime_reduce(&replacement, &child_done.state, &[])
            .await
            .unwrap();
        assert!(reset.events.is_empty());
        assert!(reset.state["items"].as_object().unwrap().is_empty());
    }
    let read: RuntimeOperation = serde_json::from_value(
        json!({"kind":"readHistory","threadId":"opaque-child-00","order":"desc"}),
    )
    .unwrap();
    let plan = provider.runtime_plan(&guarded, &read).await.unwrap();
    let plan = serde_json::to_value(plan).unwrap();
    assert_eq!(plan["action"]["kind"], "request");
    assert_eq!(
        plan["action"]["call"]["params"]["threadId"],
        "opaque-child-00"
    );
    let resume: RuntimeOperation = serde_json::from_value(json!({"kind":"resumeThread","threadId":"opaque-child-00","instructions":"","settings":guarded.settings})).unwrap();
    assert!(provider.runtime_plan(&guarded, &resume).await.is_err());
}

async fn checkpoint(
    provider: &JsProvider,
    spec: &Spec,
    context: &RuntimeContext,
    state: &Value,
    age: usize,
    report: usize,
) {
    let before = state.clone();
    let frames: Vec<RuntimeFrame> =
        serde_json::from_value(suffix(&spec.suffix, spec.suffix_text_chars)).unwrap();
    assert_eq!(frames.len(), 5);
    assert_eq!(
        (
            &context.tab_id,
            &context.thread_id,
            &context.binding_nonce,
            context.epoch,
            &context.active_turn_id
        ),
        (
            &spec.context.tab_id,
            &spec.context.thread_id,
            &spec.context.binding_nonce,
            spec.context.epoch,
            &spec.context.active_turn_id
        ),
        "aging cannot reset root binding/epoch"
    );
    let mut samples = Vec::new();
    for _ in 0..spec.repetitions {
        let mut branch = state.clone();
        let mut events = Vec::new();
        let mut micros = 0;
        for frame in &frames {
            let (result, elapsed) = step(provider, context, &branch, frame.clone()).await;
            branch = result.state;
            events.extend(result.events);
            micros += elapsed;
        }
        samples.push(micros);
        let ids: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentRuntimeEvent::ChatItem { item, .. } => Some(item.id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            vec![
                "opaque-retention-answer",
                "opaque-retention-answer",
                "opaque-retention-tool",
                "opaque-retention-tool",
                "opaque-retention-tool"
            ]
        );
        assert_eq!(
            emitted(&events, "opaque-retention-answer")["content"],
            json!("x".repeat(256))
        );
        let tool = emitted(&events, "opaque-retention-tool");
        assert_eq!(tool["toolResult"], json!("x".repeat(256)));
        assert_eq!(tool["toolStatus"], "completed");
        for event in &events {
            let value = serde_json::to_value(event).unwrap();
            assert_ne!(value["kind"], "sendEvidence");
            if value["kind"] == "turnState" {
                assert_eq!(value["state"], "working");
            }
        }
        assert_eq!(branch["activeTurnId"], json!(context.active_turn_id));
    }
    assert_eq!(state, &before);
    samples.sort_unstable();
    let length = |value: &Value| value.as_object().map_or(0, |value| value.len());
    eprintln!(
        "CHILD_RETENTION {}",
        json!({"age":age,"reportChars":report,
        "stateBytes":serde_json::to_vec(state).unwrap().len(),"contextBytes":serde_json::to_vec(context).unwrap().len(),
        "parents":length(&state["parents"]),"agents":length(&state["agents"]["rows"]),
        "open":state["open"].as_array().map_or(0, Vec::len),"retired":state["retired"].as_array().map_or(0, Vec::len),
        "suffixNativeAdapterUs":{"count":samples.len(),"p50":samples[7],"p95":samples[15],"max":samples[15]},
        "fixtureCloneAndAssertionsOutsideTiming":true})
    );
    if age == 32 {
        background_guard(provider, spec, context, state).await;
    }
    if age > 0 {
        let (late, _) = step(provider, context, state, note(context, "item/started", json!({
            "turnId":context.active_turn_id,"item":{"id":"opaque-late-start","type":"subAgentActivity","agentThreadId":"opaque-child-00","kind":"started"}
        }))).await;
        let row = emitted(&late.events, "opaque-spawn-00:opaque-child-00");
        let input = &row["toolInput"];
        assert_eq!(
            input["agent_state"], "completed",
            "completed child revived: age={age}, report={report}"
        );
        assert_eq!(input["agent_id"], "opaque-child-00");
        assert_eq!(input["agent_parent_thread_id"], json!(context.thread_id));
        assert_eq!(input["agent_ordinal"], 1);
        assert_eq!(input["agent_binding_nonce"], json!(context.binding_nonce));
        assert_eq!(input["agent_runtime_epoch"], json!(context.epoch));
        let full = "r".repeat(report);
        let actual = input["agent_result"]
            .as_str()
            .expect("late activity retains a truthful bounded report");
        if actual != full.as_str() {
            let marker = "[earlier output truncated]\n";
            assert!(
                actual.starts_with(marker)
                    && full.ends_with(&actual[marker.len()..])
                    && actual.len() < full.len()
            );
            assert!(
                !actual[marker.len()..].starts_with(marker),
                "repeat compaction must not stack truncation markers"
            );
        }
    }
}

#[tokio::test]
async fn runtime_completed_child_retention_policy() {
    let mut selected = 0;
    for input in paths() {
        let fixture = input
            .dir
            .parent()
            .unwrap()
            .join("retention/child-retention.json");
        if !fixture.is_file() {
            continue;
        }
        selected += 1;
        let spec: Spec = serde_json::from_str(&std::fs::read_to_string(&fixture).unwrap()).unwrap();
        assert_eq!(spec.schema_version, 1);
        assert_eq!(spec.provenance["kind"], "synthetic");
        for source in spec.provenance["protocolSources"].as_array().unwrap() {
            assert!(fixture
                .parent()
                .unwrap()
                .join(source.as_str().unwrap())
                .is_file());
        }
        assert_eq!(spec.ages, vec![0, 8, 32]);
        assert_eq!(spec.report_chars, vec![1024, 8192]);
        assert_eq!(spec.description_chars, 256);
        assert_eq!(spec.suffix_text_chars, 256);
        assert_eq!(spec.repetitions, 16);
        let provider = load_provider(&input.manifest, &input.declaration);
        for &report in &spec.report_chars {
            let mut context = spec.context.clone();
            let (started, _) = step(
                &provider,
                &context,
                &Value::Null,
                note(
                    &context,
                    "turn/started",
                    json!({"turn":{"id":context.active_turn_id}}),
                ),
            )
            .await;
            let mut state = started.state;
            let mut seeded = 0;
            for &age in &spec.ages {
                while seeded < age {
                    let child = format!("opaque-child-{seeded:02}");
                    context.thread_parents.push(
                        serde_json::from_value(
                            json!({"threadId":child,"parentThreadId":context.thread_id}),
                        )
                        .unwrap(),
                    );
                    let (started, _) = step(
                        &provider,
                        &context,
                        &state,
                        spawn(&context, &spec, seeded, report, false),
                    )
                    .await;
                    let (completed, _) = step(
                        &provider,
                        &context,
                        &started.state,
                        spawn(&context, &spec, seeded, report, true),
                    )
                    .await;
                    let row = emitted(
                        &completed.events,
                        &format!("opaque-spawn-{seeded:02}:opaque-child-{seeded:02}"),
                    );
                    assert!(
                        row["toolInput"]["agent_result"].as_str()
                            == Some("r".repeat(report).as_str()),
                        "terminal event lost supplied report before scratch compaction"
                    );
                    state = completed.state;
                    seeded += 1;
                }
                checkpoint(&provider, &spec, &context, &state, age, report).await;
            }
        }
    }
    assert!(selected > 0, "child retention fixture must be selected");
}

fn payload_suffix(actual: &Value, full: &str) {
    let actual = actual
        .as_str()
        .expect("bounded report remains valid Unicode after native decoding");
    let marker = "[earlier output truncated]\n";
    assert!(
        actual.len() < full.len() && actual.starts_with(marker),
        "pressure must exercise report compaction"
    );
    assert!(!actual[marker.len()..].starts_with(marker));
    assert!(
        full.ends_with(&actual[marker.len()..]),
        "retained report is an exact original suffix"
    );
    assert_eq!(
        String::from_utf16(&actual.encode_utf16().collect::<Vec<_>>()).unwrap(),
        actual
    );
}

async fn payload_pressure(full: &str) {
    assert_eq!(
        full.encode_utf16().count(),
        8192,
        "precision variants retain the declared report size"
    );
    let mut selected = 0;
    for input in paths() {
        let fixture = input
            .dir
            .parent()
            .unwrap()
            .join("retention/child-retention.json");
        if !fixture.is_file() {
            continue;
        }
        selected += 1;
        let spec: Spec = serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
        let provider = load_provider(&input.manifest, &input.declaration);
        let mut context = spec.context.clone();
        let (started, _) = step(
            &provider,
            &context,
            &Value::Null,
            note(
                &context,
                "turn/started",
                json!({"turn":{"id":context.active_turn_id}}),
            ),
        )
        .await;
        let mut state = started.state;
        let mut originals = Vec::new();
        for index in 0..32 {
            let child = format!("opaque-child-{index:02}");
            context.thread_parents.push(
                serde_json::from_value(
                    json!({"threadId":child,"parentThreadId":context.thread_id}),
                )
                .unwrap(),
            );
            let (started, _) = step(
                &provider,
                &context,
                &state,
                spawn(&context, &spec, index, 0, false),
            )
            .await;
            let mut frame = spawn(&context, &spec, index, 0, true);
            if let RuntimeFrame::Notification { params, .. } = &mut frame {
                params["item"]["agentsStates"][&child]["message"] = json!(full);
            } else {
                panic!("expected child completion notification");
            }
            let (completed, _) = step(&provider, &context, &started.state, frame).await;
            assert!(serde_json::to_vec(&completed).unwrap().len() <= MAX_RUNTIME_VALUE_BYTES);
            assert!(serde_json::to_vec(&completed.state).unwrap().len() <= 192 * 1024);
            let row = emitted(
                &completed.events,
                &format!("opaque-spawn-{index:02}:{child}"),
            );
            assert_eq!(row["toolInput"]["agent_result"], full);
            let serialized = serde_json::to_vec(&completed.events).unwrap();
            originals.push((completed.events, serialized));
            state = completed.state;
        }
        for _ in 0..3 {
            let scratch = &state["items"]["opaque-spawn-00:opaque-child-00"]["toolInput"];
            payload_suffix(&scratch["agent_result"], full);
            assert!(
                serde_json::to_vec(scratch).unwrap().len() <= 8192,
                "compacted input fits the initial pressure cap"
            );
            let (late, _) = step(&provider, &context, &state, note(&context, "item/started", json!({
                "turnId":context.active_turn_id,"item":{"id":"opaque-late-start","type":"subAgentActivity","agentThreadId":"opaque-child-00","kind":"started"}
            }))).await;
            assert!(serde_json::to_vec(&late.state).unwrap().len() <= 192 * 1024);
            assert!(serde_json::to_vec(&late).unwrap().len() <= MAX_RUNTIME_VALUE_BYTES);
            let row = emitted(&late.events, "opaque-spawn-00:opaque-child-00");
            let input = &row["toolInput"];
            assert_eq!(input["agent_state"], "completed");
            assert_eq!(input["agent_id"], "opaque-child-00");
            assert_eq!(input["agent_parent_thread_id"], json!(context.thread_id));
            assert_eq!(input["agent_ordinal"], 1);
            assert_eq!(input["agent_binding_nonce"], context.binding_nonce);
            assert_eq!(input["agent_runtime_epoch"], context.epoch);
            payload_suffix(&input["agent_result"], full);
            state = late.state;
            for (events, serialized) in &originals {
                assert_eq!(&serde_json::to_vec(events).unwrap(), serialized);
            }
        }
    }
    assert!(
        selected > 0,
        "agent payload precision fixtures must be selected"
    );
}

#[tokio::test]
async fn runtime_child_payload_unicode_suffix() {
    payload_pressure(&"😀".repeat(4096)).await;
    payload_pressure(&format!("a{}a", "😀".repeat(4095))).await;
}

#[tokio::test]
async fn runtime_child_payload_json_escaping() {
    payload_pressure(&"\0\n\"\\".repeat(2048)).await;
}

#[tokio::test]
async fn runtime_child_payload_repeated_marker() {
    let marker = "[earlier output truncated]\n";
    payload_pressure(&format!("{marker}{}", "r".repeat(8192 - marker.len()))).await;
}
