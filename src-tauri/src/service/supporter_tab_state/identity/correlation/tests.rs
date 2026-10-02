use super::*;
use crate::service::supporter_tab_state::identity::{self, DecisionSource, LocalIdentity};
use crate::service::supporter_tab_state::{coordinator::ActionOwner, ObservationTrigger};
use serde_json::json;

fn local(nonce: &str, session: Option<&str>) -> LocalIdentity {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.id = "fixture-tab".into();
    tab.pty_id = Some("reused-pty".into());
    tab.session_binding_nonce = Some(nonce.into());
    tab.agent_session_id = session.map(str::to_owned);
    LocalIdentity::of_tab(tab)
}

fn capture(
    local: &LocalIdentity,
    point: ObservationTrigger,
    id: u64,
    kind: Option<&str>,
) -> Correlation {
    let owner = ActionOwner::default();
    let observation = owner.observe("fixture-tab", &local.launch_key, Some(11), 22);
    Correlation::capture(
        Some(local),
        &local.launch_key,
        point,
        observation.occurrence,
        id,
        22,
        kind,
    )
}

fn record(correlation: &Correlation) -> Value {
    identity::journal(
        "fixture-tab",
        RecordIdentity::captured(41, correlation),
        DecisionSource::Adapter,
        "provider_update_prompt",
        Some(0.95),
        "fixture-title",
    )
}

#[test]
fn supporter_first_opinion_passive_same_occurrence_distinct_capture_identity() {
    let local = local("nonce-a", Some("claimed-session"));
    let first = capture(
        &local,
        ObservationTrigger::PromptCandidate,
        41,
        Some("update"),
    );
    let second = capture(
        &local,
        ObservationTrigger::PromptCandidate,
        42,
        Some("update"),
    );
    let first_record = record(&first);
    let second_record = identity::journal(
        "fixture-tab",
        RecordIdentity::captured(42, &second),
        DecisionSource::Adapter,
        "provider_update_prompt",
        Some(0.95),
        "fixture-title",
    );
    for field in [
        "launch_id",
        "session_id",
        "occurrence_id",
        "frame_hash",
        "decision_point",
    ] {
        assert_eq!(
            first_record["correlation"][field],
            second_record["correlation"][field]
        );
    }
    assert_ne!(
        first_record["observation_id"],
        second_record["observation_id"]
    );
    assert_eq!(first_record["correlation"]["observation_id"], 41);
    assert_eq!(first_record["decision_point"], "prompt_candidate");
    assert_eq!(first_record["observation_status"], "completed");
    assert_eq!(
        first_record["correlation"]["session_id"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(project(&first_record).unwrap(), first_record["correlation"]);
    assert_eq!(record(&first), first_record);
}

#[test]
fn supporter_first_opinion_passive_reused_pty_nonce_and_claimed_session_are_scoped() {
    let a = capture(
        &local("nonce-a", Some("session-a")),
        ObservationTrigger::PromptCandidate,
        41,
        None,
    );
    let b = capture(
        &local("nonce-b", Some("session-a")),
        ObservationTrigger::PromptCandidate,
        41,
        None,
    );
    let c = capture(
        &local("nonce-a", Some("session-b")),
        ObservationTrigger::PromptCandidate,
        41,
        None,
    );
    let a = record(&a);
    let b = record(&b);
    let c = record(&c);
    assert_ne!(a["correlation"]["launch_id"], b["correlation"]["launch_id"]);
    assert_eq!(
        a["correlation"]["session_id"],
        b["correlation"]["session_id"]
    );
    assert_eq!(a["correlation"]["launch_id"], c["correlation"]["launch_id"]);
    assert_ne!(
        a["correlation"]["session_id"],
        c["correlation"]["session_id"]
    );
    assert_ne!(
        a["correlation"]["occurrence_id"],
        b["correlation"]["occurrence_id"]
    );
    assert_ne!(
        a["correlation"]["occurrence_id"],
        c["correlation"]["occurrence_id"]
    );
    let unclaimed = capture(
        &local("nonce-a", None),
        ObservationTrigger::PromptCandidate,
        41,
        None,
    );
    assert!(record(&unclaimed)["correlation"]["session_id"].is_null());
}

#[test]
fn supporter_first_opinion_passive_wrong_point_id_or_untrusted_scope_is_rejected() {
    let c = capture(
        &local("nonce-a", Some("session-a")),
        ObservationTrigger::PromptCandidate,
        41,
        None,
    );
    let original = record(&c);
    for (field, value) in [
        ("decision_point", json!("tab_exit")),
        ("observation_id", json!(42)),
        ("version", json!(2)),
        ("launch_id", json!("token=private-scope")),
        ("frame_hash", json!("invalid")),
        ("screen", json!("private-screen")),
    ] {
        let mut changed = original.clone();
        changed["correlation"][field] = value;
        assert!(project(&changed).is_none(), "field {field}");
    }
    let mut wrong_point = original;
    wrong_point["decision_point"] = json!("worker_idle_without_report");
    assert!(project(&wrong_point).is_none());
}

#[test]
fn supporter_first_opinion_passive_candidate_evidence_does_not_invent_an_alternate_verdict() {
    let c = capture(
        &local("nonce-a", Some("session-a")),
        ObservationTrigger::PromptCandidate,
        41,
        Some("onboarding"),
    );
    let record = record(&c);
    assert_eq!(
        record["correlation"]["candidate_evidence"],
        json!({"status":"observed", "source":"provider_safe_target", "kind":"onboarding"})
    );
    assert_eq!(record["alternate_verdict"], json!({"status":"skipped"}));
    let unavailable = capture(
        &local("nonce-a", None),
        ObservationTrigger::PromptCandidate,
        41,
        None,
    );
    assert_eq!(
        record_fn(&unavailable)["correlation"]["candidate_evidence"],
        json!({"status":"unavailable"})
    );
    for source in [
        DecisionSource::Error,
        DecisionSource::Budget,
        DecisionSource::Timeout,
        DecisionSource::NoAdapter,
    ] {
        let rejected = identity::journal(
            "fixture-tab",
            RecordIdentity::captured(41, &c),
            source,
            "other",
            None,
            "fixture-title",
        );
        assert_ne!(rejected["observation_status"], "completed");
        assert_eq!(
            rejected["alternate_verdict"],
            json!({"status":"unavailable"})
        );
        assert_eq!(rejected["model_verdict"], json!({"status":"unavailable"}));
    }
}

fn record_fn(c: &Correlation) -> Value {
    record(c)
}

#[test]
fn supporter_first_opinion_passive_raw_identity_and_context_secrets_never_serialize() {
    let nonce = "private-launch-nonce";
    let session = "private-claimed-session";
    let c = capture(
        &local(nonce, Some(session)),
        ObservationTrigger::TabExit,
        41,
        None,
    );
    let record = record(&c);
    let serialized = record.to_string();
    for private in [nonce, session, "reused-pty"] {
        assert!(!serialized.contains(private));
    }
    assert_eq!(record["decision_point"], "tab_exit");
    assert_eq!(record["correlation"]["frame_hash"], "0000000000000016");
    let no_capture = identity::journal(
        "fixture-tab",
        41,
        DecisionSource::NoAdapter,
        "other",
        None,
        "fixture-title",
    );
    assert_eq!(no_capture["decided_by"], "fallback(no_adapter)");
    assert_eq!(no_capture["observation_status"], "unavailable");
    assert!(no_capture.get("correlation").is_none());
}
