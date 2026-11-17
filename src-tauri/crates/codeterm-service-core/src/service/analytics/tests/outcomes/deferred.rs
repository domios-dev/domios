use super::*;

#[tokio::test]
async fn committed_canvas_file_actions_exclude_frames_conflicts_refusals_and_optout() {
    use codeterm_local_preview::html_preview::{preview_test_root, write_canvas_file, CanvasWrite};
    let sink = Sink::new(Consent::On).await;
    let path = preview_test_root(sink.dir.path()).join("board.canvas.json");
    let path = path.to_str().unwrap();
    let initial = r#"{"format":"domios.canvas/1","items":[{"id":"anchor","kind":"note","markdown":"private"}]}"#;
    std::fs::write(path, initial).unwrap();
    let created = initial.replace(
        "\"items\":[",
        "\"items\":[{\"id\":\"new\",\"kind\":\"note\",\"markdown\":\"new\"},",
    );
    let updated = created.replace("\"markdown\":\"new\"", "\"markdown\":\"edited\"");
    let moved = updated.replace("\"id\":\"new\"", "\"id\":\"new\",\"x\":40,\"y\":20");
    let persist = |before: &str, after: &str| {
        let outcome = write_canvas_file(path, |_, _| Ok(()), before, after);
        for event in super::super::super::actions::canvas_file_events(&outcome, before, after) {
            sink.worker.record_nowait(event).unwrap();
        }
        outcome
    };
    assert_eq!(persist(initial, &created), Ok(CanvasWrite::Written));
    assert!(matches!(
        persist(initial, &updated),
        Ok(CanvasWrite::Conflict(_))
    ));
    assert_eq!(persist(&created, &updated), Ok(CanvasWrite::Written));
    assert_eq!(persist(&updated, &moved), Ok(CanvasWrite::Written));
    assert_eq!(persist(&moved, initial), Ok(CanvasWrite::Written));
    assert_eq!(persist(initial, initial), Ok(CanvasWrite::Written));
    let refused = write_canvas_file(
        path,
        |_, _| Err("private refusal".into()),
        initial,
        &created,
    );
    assert!(refused.is_err());
    assert!(
        super::super::super::actions::canvas_file_events(&refused, initial, &created).is_empty()
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), initial);
    assert_eq!(
        sink.events().await,
        vec![
            Event::CanvasElementSaved {
                operation: codeterm_analytics::CanvasOperation::Create,
                kind: codeterm_analytics::CanvasKind::Note
            },
            Event::CanvasElementSaved {
                operation: codeterm_analytics::CanvasOperation::Update,
                kind: codeterm_analytics::CanvasKind::Note
            },
            Event::CanvasElementSaved {
                operation: codeterm_analytics::CanvasOperation::Delete,
                kind: codeterm_analytics::CanvasKind::Note
            },
        ]
    );
    sink.worker.set_consent(Consent::Off).await.unwrap();
    assert_eq!(persist(initial, &created), Ok(CanvasWrite::Written));
    assert_eq!(sink.events().await.len(), 3);
}

#[tokio::test]
async fn committed_canvas_actions_exclude_layout_noops_failures_and_optout() {
    use codeterm_plan::{plan::OrchestratorRef, service_plan::PlanStore};
    let sink = Sink::new(Consent::On).await;
    let store = PlanStore::new_for_test(sink.dir.path());
    let plan = store
        .create_plan_for_team(
            "private".into(),
            sink.dir.path().display().to_string(),
            OrchestratorRef::from_tab("private"),
            "private-team".into(),
            vec![],
            1,
        )
        .unwrap();
    store.bind_element_observer(super::super::super::actions::element_observer(
        sink.worker.clone(),
    ));
    assert!(store
        .add_note("missing", "private", vec![], None, None, 2)
        .is_err());
    let id = store
        .add_note(&plan.id, "private", vec![], None, None, 3)
        .unwrap();
    store
        .set_layout(&plan.id, &id, Some(10.0), Some(20.0), 4)
        .unwrap();
    store
        .update_element(&plan.id, &id, None, None, None, 5)
        .unwrap();
    store
        .update_element(&plan.id, &id, Some("private-kind"), None, None, 6)
        .unwrap();
    store.rm_element(&plan.id, &id, 7).unwrap();
    assert!(store.rm_element(&plan.id, &id, 8).is_err());
    assert_eq!(
        sink.events().await,
        vec![
            Event::CanvasElementSaved {
                operation: codeterm_analytics::CanvasOperation::Create,
                kind: codeterm_analytics::CanvasKind::Note
            },
            Event::CanvasElementSaved {
                operation: codeterm_analytics::CanvasOperation::Update,
                kind: codeterm_analytics::CanvasKind::Other
            },
            Event::CanvasElementSaved {
                operation: codeterm_analytics::CanvasOperation::Delete,
                kind: codeterm_analytics::CanvasKind::Other
            },
        ]
    );
    sink.worker.set_consent(Consent::Off).await.unwrap();
    store
        .add_note(&plan.id, "private", vec![], None, None, 9)
        .unwrap();
    assert_eq!(sink.events().await.len(), 3);
}

#[tokio::test]
async fn committed_plugin_booleans_exclude_save_failures_noops_and_foreign_config() {
    use crate::config::{product_observer, save_config_and_apply_with, schema::AppConfig};
    let sink = Sink::new(Consent::On).await;
    let state = Arc::new(parking_lot::Mutex::new(AppConfig::default()));
    product_observer::bind_plugin_enabled_observer(
        &state,
        super::super::super::actions::plugin_observer(sink.worker.clone()),
    );
    let mut next = state.lock().clone();
    next.plugins.insert(
        "private-plugin".into(),
        serde_json::json!({"enabled":false,"secret":"private"}),
    );
    assert!(
        save_config_and_apply_with(&state, next.clone(), |_| Err("private error".into())).is_err()
    );
    assert!(sink.events().await.is_empty());
    save_config_and_apply_with(&state, next.clone(), |_| Ok(())).unwrap();
    save_config_and_apply_with(&state, next.clone(), |_| Ok(())).unwrap();
    let foreign = parking_lot::Mutex::new(AppConfig::default());
    save_config_and_apply_with(&foreign, next, |_| Ok(())).unwrap();
    assert_eq!(
        sink.events().await,
        vec![Event::PluginEnabledChanged { enabled: false }]
    );
}
