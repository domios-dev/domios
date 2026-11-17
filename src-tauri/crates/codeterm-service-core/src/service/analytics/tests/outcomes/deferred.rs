use super::*;

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
