use super::*;
use serde_json::json;

fn job(id: &str, scenario: &str, creator: &str, team: Option<&str>, stopped: bool) -> Sentinel {
    let mut job: Sentinel = serde_json::from_value(json!({
        "id": id, "workspace_id": "workspace", "team_id": team, "creator_tab_id": creator,
        "cwd": "/tmp", "probe": "status", "question": "Ready?", "buckets": ["ready"],
        "routes": {"ready": []}, "matches": {}, "interval_ms": 10000, "next_run": 0, "expires_at": null,
        "until": null, "last_reason": null,
        "machine": {"paused": false, "stopped": stopped, "last_hash": null, "last_bucket": null}
    }))
    .unwrap();
    job.scenario_id = Some(scenario.into());
    job
}

fn caller(tab: &str, team_owner: bool) -> DeleteCaller<'_> {
    DeleteCaller {
        tab,
        workspace: "workspace",
        team: Some("team"),
        team_owner,
    }
}

fn fleet() -> Vec<Sentinel> {
    vec![
        job("root-a", "root-a", "creator", Some("team"), true),
        job("stage-a", "root-a", "creator", Some("team"), true),
        job("root-b", "root-b", "creator", Some("team"), false),
        job("root-c", "root-c", "other", Some("team"), true),
        job("root-d", "root-d", "creator", Some("other-team"), true),
    ]
}

#[test]
fn blueprint_delete_resolves_a_stage_to_its_whole_scenario() {
    let roots = plan_delete(
        &fleet(),
        &caller("creator", false),
        DeleteTarget::One("stage-a"),
        false,
    )
    .unwrap();
    assert_eq!(roots, vec!["root-a".to_string()]);
}

#[test]
fn blueprint_delete_refuses_a_running_blueprint_unless_stop_is_passed() {
    let error = plan_delete(
        &fleet(),
        &caller("creator", false),
        DeleteTarget::One("root-b"),
        false,
    )
    .unwrap_err();
    assert!(error.contains("--stop"), "{error}");
    let roots = plan_delete(
        &fleet(),
        &caller("creator", false),
        DeleteTarget::One("root-b"),
        true,
    )
    .unwrap();
    assert_eq!(roots, vec!["root-b".to_string()]);
}

#[test]
fn blueprint_delete_allows_only_the_creator_or_the_team_owner() {
    let refused = plan_delete(
        &fleet(),
        &caller("creator", false),
        DeleteTarget::One("root-c"),
        false,
    )
    .unwrap_err();
    assert!(
        refused.contains("creator tab or the Team owner"),
        "{refused}"
    );
    let owner = plan_delete(
        &fleet(),
        &caller("orchestrator", true),
        DeleteTarget::One("root-c"),
        false,
    )
    .unwrap();
    assert_eq!(owner, vec!["root-c".to_string()]);
}

#[test]
fn blueprint_delete_is_team_scoped() {
    let error = plan_delete(
        &fleet(),
        &caller("creator", true),
        DeleteTarget::One("root-d"),
        false,
    )
    .unwrap_err();
    assert!(error.contains("not found in this Team"), "{error}");
}

#[test]
fn blueprint_delete_stopped_selects_only_stopped_blueprints_the_caller_may_delete() {
    assert_eq!(
        plan_delete(
            &fleet(),
            &caller("creator", false),
            DeleteTarget::Stopped,
            false
        )
        .unwrap(),
        vec!["root-a".to_string()]
    );
    assert_eq!(
        plan_delete(
            &fleet(),
            &caller("orchestrator", true),
            DeleteTarget::Stopped,
            false
        )
        .unwrap(),
        vec!["root-a".to_string(), "root-c".to_string()]
    );
}

#[test]
fn blueprint_delete_stages_scenario_removal_in_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    for job in fleet() {
        store.insert(job).unwrap();
    }
    let staged = stage_delete(
        &mut store,
        &caller("creator", false),
        DeleteTarget::One("root-a"),
        false,
    )
    .unwrap();
    assert_eq!(staged.len(), 1);
    for (_, staged) in staged {
        staged.into_ready().unwrap();
    }
    let left: Vec<_> = store.list().iter().map(|job| job.id.as_str()).collect();
    assert_eq!(left, vec!["root-b", "root-c", "root-d"]);
}
