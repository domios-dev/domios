use crate::service::session_model::*;

#[test]
fn runtime_model_mismatch_records_actual_and_structured_reason() {
    let update = model_identity_update(
        None,
        Some("planned-codex"),
        Some("gpt-5.6-luna"),
        Some("gpt-5.6-luna"),
        None,
        "gpt-5.6-terra",
        false,
    );

    assert!(update.model_changed);
    let mismatch = update.mismatch.expect("mismatch must be structured");
    assert_eq!(mismatch.requested_model, "gpt-5.6-luna");
    assert_eq!(mismatch.planned_model.as_deref(), Some("planned-codex"));
    assert_eq!(
        mismatch.provider_resolved_model.as_deref(),
        Some("gpt-5.6-luna")
    );
    assert_eq!(mismatch.actual_model, "gpt-5.6-terra");
    assert_eq!(mismatch.reason, "runtime_session_model_mismatch");
    let fallback = update.fallback.expect("fallback must be surfaced");
    assert_eq!(fallback.actual_model.as_deref(), Some("gpt-5.6-terra"));
}

fn launch_report() -> crate::multiplexer::types::LaunchModelReport {
    crate::multiplexer::types::LaunchModelReport {
        model: "next-model-b2".into(),
        transcript_model_at_launch: Some("prior-model-a1".into()),
    }
}

#[test]
fn launch_report_outranks_a_transcript_that_predates_the_launch() {
    for transcript in [Some("prior-model-a1".to_string()), None] {
        let (model, kept) =
            session_model_evidence(Some(launch_report()), transcript).expect("evidence");
        assert_eq!(model, "next-model-b2");
        assert_eq!(kept, Some(launch_report()));
    }
}

#[test]
fn transcript_past_the_launch_retires_the_launch_report() {
    let (model, kept) =
        session_model_evidence(Some(launch_report()), Some("later-model-c3".into()))
            .expect("evidence");
    assert_eq!(model, "later-model-c3");
    assert_eq!(kept, None);
    assert_eq!(session_model_evidence(None, None), None);
}

#[test]
fn repeated_runtime_mismatch_keeps_actual_without_second_model_change() {
    let update = model_identity_update(
        Some("gpt-5.6-terra"),
        None,
        Some("gpt-5.6-luna"),
        Some("gpt-5.6-luna"),
        None,
        "gpt-5.6-terra",
        false,
    );

    assert!(!update.model_changed);
    assert_eq!(
        update.mismatch,
        Some(crate::multiplexer::types::ModelIdentityMismatch {
            planned_model: None,
            requested_model: "gpt-5.6-luna".into(),
            provider_resolved_model: Some("gpt-5.6-luna".into()),
            actual_model: "gpt-5.6-terra".into(),
            reason: "runtime_session_model_mismatch".into(),
        })
    );
}

#[test]
fn matching_runtime_model_clears_mismatch_shape() {
    let update = model_identity_update(
        Some("gpt-5.6-luna"),
        None,
        Some("gpt-5.6-luna"),
        Some("gpt-5.6-luna"),
        None,
        "GPT-5.6-LUNA",
        true,
    );

    assert!(!update.model_changed);
    assert!(update.mismatch.is_none());
    assert!(update.fallback.is_none());
}

#[test]
fn fallback_without_provider_resolution_uses_observed_model() {
    let update = model_identity_update(None, None, Some("sonnet"), None, None, "opus", false);

    assert_eq!(
        update.mismatch.as_ref().unwrap().provider_resolved_model,
        None
    );
    assert_eq!(
        update.fallback.unwrap().provider_resolved_model,
        "opus",
        "a fallback must not label the request as the provider-resolved model"
    );
}

#[test]
fn session_change_clears_stale_actual_but_keeps_launch_provenance() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_planned_model = Some("codex/gpt-planned".into());
    tab.agent_requested_model = Some("gpt-requested".into());
    tab.agent_provider_resolved_model = Some("gpt-resolved".into());
    tab.agent_session_model = Some("gpt-actual-old".into());
    tab.model_identity_mismatch = Some(crate::multiplexer::types::ModelIdentityMismatch {
        planned_model: Some("codex/gpt-planned".into()),
        requested_model: "gpt-requested".into(),
        provider_resolved_model: Some("gpt-resolved".into()),
        actual_model: "gpt-actual-old".into(),
        reason: "runtime_session_model_mismatch".into(),
    });

    clear_runtime_model_evidence(tab);

    assert_eq!(
        tab.agent_planned_model.as_deref(),
        Some("codex/gpt-planned")
    );
    assert_eq!(tab.agent_requested_model.as_deref(), Some("gpt-requested"));
    assert_eq!(
        tab.agent_provider_resolved_model.as_deref(),
        Some("gpt-resolved")
    );
    assert!(tab.agent_session_model.is_none());
    assert!(tab.model_identity_mismatch.is_none());
}

#[test]
fn delayed_runtime_model_result_cannot_cross_session_boundary() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("provider-a".into());
    tab.agent_session_id = Some("session-new".into());

    assert!(!runtime_model_update_matches_session(
        tab,
        "provider-a",
        "session-old"
    ));
    assert!(!runtime_model_update_matches_session(
        tab,
        "provider-b",
        "session-new"
    ));
    assert!(runtime_model_update_matches_session(
        tab,
        "provider-a",
        "session-new"
    ));
}
