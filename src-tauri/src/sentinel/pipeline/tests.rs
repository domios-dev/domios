use super::effect_after_claim;
use super::*;
use crate::cron::clock::MockClock;
use crate::multiplexer::{state::MultiplexerState, types::AgentRole};
use crate::sentinel::routes::pipeline_result_owner;
use crate::sentinel::store::{Owner, Store};
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::blueprint_pipeline::{
    BlueprintOwner, BlueprintResult, BlueprintSpec, IoType, ResultTarget, Step, StepKind, Trigger,
};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[test]
fn blueprint_pipeline_probe_capacity_preserves_terminal_reason() {
    let result = command_failure(
        probe::process::ProbeFailure::new("probe_owner_capacity"),
        "input",
    );
    assert_eq!(result.outcome, StepOutcome::Failed);
    assert_eq!(result.state, "failed");
    assert_eq!(result.reason.as_deref(), Some("probe_owner_capacity"));
    assert_eq!(result.next_text, "input");
    assert!(!result.uncertain);
}

#[test]
fn blueprint_pipeline_effect_runs_after_one_successful_claim() {
    let events = RefCell::new(Vec::new());
    let result = effect_after_claim(
        || {
            events.borrow_mut().push("claim");
            true
        },
        || {
            events.borrow_mut().push("effect");
            7
        },
    );

    assert_eq!(result, Some(7));
    assert_eq!(events.into_inner(), ["claim", "effect"]);
}

#[test]
fn blueprint_pipeline_refused_claim_skips_the_effect() {
    let events = RefCell::new(Vec::new());
    let result = effect_after_claim(
        || {
            events.borrow_mut().push("claim");
            false
        },
        || {
            events.borrow_mut().push("effect");
        },
    );

    assert_eq!(result, None);
    assert_eq!(events.into_inner(), ["claim"]);
}

fn result_spec(owner: BlueprintOwner, target: ResultTarget) -> BlueprintSpec {
    BlueprintSpec {
        name: "result-delivery-test".into(),
        owner,
        trigger: Trigger::Schedule {
            every: "20s".into(),
        },
        steps: vec![Step {
            name: "emit".into(),
            input: IoType::Text,
            kind: StepKind::Command {
                command: "echo result".into(),
            },
            on: BTreeMap::new(),
        }],
        result: BlueprintResult { to: target },
    }
}

fn result_job(spec: BlueprintSpec, owner: Owner) -> crate::sentinel::store::Sentinel {
    let directory = tempfile::tempdir().unwrap();
    Store::new(
        directory.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    )
    .prepare_pipeline(spec, owner, true)
    .unwrap()
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn blueprint_delivery_result_targets_persist_before_delivery_without_resend() {
    let (mut mux, creator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&creator)
        .unwrap()
        .team_id;
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    mux.get_tab_mut(&creator).unwrap().agent_type = Some("test-agent".into());
    let named = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&named, AgentRole::Worker, Some(&creator))
        .unwrap();
    mux.get_tab_mut(&named).unwrap().agent_type = Some("test-agent".into());
    let fixture = ServiceFixture::new(mux);
    let owner = Owner {
        creator_tab_id: creator.clone(),
        team_id: Some(team_id.clone()),
        workspace_id: workspace,
        cwd: ".".into(),
    };
    let verdict = Verdict {
        bucket: "done".into(),
        confidence: 1.0,
    };
    let team_owner = Owner {
        creator_tab_id: named.clone(),
        ..owner.clone()
    };
    let cases = [
        (
            result_spec(BlueprintOwner::Tab, ResultTarget::Owner),
            owner.clone(),
            creator.as_str(),
            "owner result",
            true,
        ),
        (
            result_spec(BlueprintOwner::Tab, ResultTarget::Tab(named.clone())),
            owner.clone(),
            named.as_str(),
            "named result",
            false,
        ),
        (
            result_spec(BlueprintOwner::Team, ResultTarget::Team),
            team_owner,
            creator.as_str(),
            "team result",
            false,
        ),
    ];
    let mut attempted_targets = Vec::new();

    for (spec, job_owner, expected_target, output, pending) in cases {
        let job = result_job(spec.clone(), job_owner);
        let routes = result_routes(&fixture.service, &job, &spec).unwrap();
        assert_eq!(routes, [Route::Send(expected_target.into())]);
        let body_owner = pipeline_result_owner(&fixture.service, &job, &spec);
        assert_eq!(body_owner, creator);
        let (path, body_ref) = super::super::routes::write_pipeline_result_body(
            &fixture.service,
            &body_owner,
            &job,
            &verdict,
            output,
        )
        .unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains(output));
        let attempts_before = attempted_targets.len();
        let result = super::super::routes::deliver_pipeline_result_with(
            expected_target,
            &format!("Read {}", path.display()),
            body_ref.clone(),
            |target, packet| {
                assert!(path.is_file());
                assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
                assert!(packet.contains(path.to_str().unwrap()));
                attempted_targets.push(target.to_string());
                if pending {
                    Ok(crate::service::messenger::Delivery::Pending {
                        delivery_id: "parked-result".into(),
                    })
                } else {
                    Ok(crate::service::messenger::Delivery::Pty {
                        text: packet.into(),
                    })
                }
            },
        );
        assert_eq!(result.body_ref.as_deref(), Some(body_ref.as_str()));
        assert_eq!(
            result.state,
            if pending { "unconfirmed" } else { "delivered" }
        );
        assert_eq!(attempted_targets.len(), attempts_before + 1);
    }

    assert_eq!(
        attempted_targets,
        [creator.clone(), named.clone(), creator.clone()]
    );

    let notify_spec = result_spec(BlueprintOwner::Tab, ResultTarget::Notify);
    let notify_job = result_job(notify_spec.clone(), owner);
    assert_eq!(
        result_routes(&fixture.service, &notify_job, &notify_spec).unwrap(),
        [Route::Notify]
    );
    let body_owner = pipeline_result_owner(&fixture.service, &notify_job, &notify_spec);
    let (path, body_ref) = super::super::routes::write_pipeline_result_body(
        &fixture.service,
        &body_owner,
        &notify_job,
        &verdict,
        "notification result",
    )
    .unwrap();
    let notification = super::super::routes::notify_pipeline_result(
        &fixture.service,
        &notify_job,
        &verdict,
        &path,
        body_ref.clone(),
    );
    assert_eq!(notification.state, "delivered");
    assert_eq!(notification.body_ref.as_deref(), Some(body_ref.as_str()));
    assert!(path.is_file());
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .ends_with("notification result"));
    assert!(fixture.service.notif.lock().list(false).is_empty());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn blueprint_delivery_unresolvable_named_target_fails_at_fire_with_reason() {
    let (mut mux, creator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&creator)
        .unwrap()
        .team_id;
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    mux.get_tab_mut(&creator).unwrap().agent_type = Some("test-agent".into());
    let fixture = ServiceFixture::new(mux);
    let spec = result_spec(
        BlueprintOwner::Tab,
        ResultTarget::Tab("missing_target".into()),
    );
    let owner = Owner {
        creator_tab_id: creator,
        team_id: Some(team_id),
        workspace_id: workspace,
        cwd: ".".into(),
    };
    let job = result_job(spec.clone(), owner);
    let verdict = Verdict {
        bucket: "done".into(),
        confidence: 1.0,
    };
    let (body_path, body_ref) = super::super::routes::write_pipeline_result_body(
        &fixture.service,
        &job.creator_tab_id,
        &job,
        &verdict,
        "result before unresolved route",
    )
    .unwrap();

    let failure = result_routes(&fixture.service, &job, &spec).unwrap_err();
    let journal = unresolved_target_finish("run_unresolved", 1000, &body_ref);

    assert!(failure.contains("target not found"));
    assert_eq!(
        failure.as_str(),
        "blueprint send target not found: missing_target"
    );
    assert!(body_path.is_file());
    let BlueprintJournalWrite::Finish {
        outcome,
        result_summary,
        body_ref: stored_body_ref,
        reason,
        ..
    } = journal
    else {
        panic!("unresolved target must finish as a durable result row");
    };
    assert_eq!(outcome, "handler_failed");
    assert_eq!(result_summary, "result target unresolved");
    assert_eq!(stored_body_ref.as_deref(), Some(body_ref.as_str()));
    assert_eq!(reason.as_deref(), Some("result_target_unresolved"));
}

#[test]
fn blueprint_pipeline_journal_write_failure_closes_once_as_failed() {
    let closed = RefCell::new(Vec::new());
    with_run_close(
        || {
            required_journal(false)?;
            Ok(RunCompletion::new(JournalState::Done, "done"))
        },
        |completion| closed.borrow_mut().push(completion.clone()),
    );
    assert_eq!(
        closed.into_inner(),
        vec![RunCompletion::new(
            JournalState::Failed,
            JournalReason::JournalWriteFailed
        )]
    );
}

#[test]
fn blueprint_pipeline_uncertain_effect_closes_once_as_held_with_typed_reason() {
    let mut failure = probe::process::ProbeFailure::new("test_effect");
    failure.unconfirmed(probe::process::ResourceKind::Child);
    let result = command_failure(failure, "input");
    assert!(result.uncertain);
    let closed = RefCell::new(Vec::new());
    with_run_close(
        || Ok(RunCompletion::held(JournalReason::UncertainEffect)),
        |completion| closed.borrow_mut().push(completion.clone()),
    );
    assert_eq!(
        closed.into_inner(),
        vec![RunCompletion::held(JournalReason::UncertainEffect)]
    );
}
