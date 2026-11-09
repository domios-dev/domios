use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::TaskStatus;
use crate::service::plan::load_audit::{execution_plan, seed, task};
use crate::service::test_fixture::ServiceFixture;

#[test]
fn sql_failure_after_digest_facts_leaves_rate_rule_available_for_the_next_round() {
    let (mux, owner) = MultiplexerState::new();
    let fx = ServiceFixture::new(mux);
    let svc = &fx.service;
    {
        let mut config = svc.ext::<crate::config::ConfigState>().lock();
        config.orchestration.map.enabled = false;
        config.usage.enabled = false;
        config.orchestration.digests.enabled = true;
        config.orchestration.digests.rules = vec![crate::config::schema::DigestRuleConfig {
            id: "pressure".into(),
            enabled: true,
            threshold: 0,
            template: "opaque queue {in_review}".into(),
            ..Default::default()
        }];
    }
    let id = seed(
        &svc.plan_store,
        execution_plan(
            fx.temp.path(),
            "digest",
            &owner,
            vec![task("review", TaskStatus::InReview, None)],
        ),
    );
    let connection =
        rusqlite::Connection::open(fx.temp.path().join(".domios").join("plans.db")).unwrap();
    connection.execute("UPDATE plan_heartbeat SET row_json=json_set(row_json,'$.learnings',42) WHERE plan_id=?1",[&id]).unwrap();
    let snapshot = svc
        .plan_store
        .reminder_read_snapshot(crate::service::hierarchy::heartbeat_gate::scrubbed)
        .unwrap();
    let read = snapshot.composition(&id).unwrap().unwrap();
    let inventory = snapshot_notice_inventory(svc, &id, None, Some(&owner));
    let now = now_epoch_ms();
    let input = read.digest_input(now, &inventory.targets()).unwrap();
    let prepared = prepare_digest_lines_for_input(svc, &input, &inventory, Some(&owner));
    assert_eq!(prepared.rate_marks.len(), 1);
    assert!(read
        .compose_owner("directive", &prepared.lines, None, false, now)
        .is_err());
    assert!(svc
        .plan_turn_counter
        .digest_rule_due(&owner, "pressure", now_epoch_secs(), 3600));
    assert!(
        deliver_orchestrator_notice_exact(svc, &id, &owner, "system", "opaque payload").is_err()
    );
    assert!(svc
        .plan_turn_counter
        .digest_rule_due(&owner, "pressure", now_epoch_secs(), 3600));
    drop(read);
    drop(snapshot);
    connection.execute("UPDATE plan_heartbeat SET row_json=json_set(row_json,'$.learnings',json('[]')) WHERE plan_id=?1",[&id]).unwrap();
    let snapshot = svc
        .plan_store
        .reminder_read_snapshot(crate::service::hierarchy::heartbeat_gate::scrubbed)
        .unwrap();
    let read = snapshot.composition(&id).unwrap().unwrap();
    let input = read.digest_input(now, &inventory.targets()).unwrap();
    let retry = prepare_digest_lines_for_input(svc, &input, &inventory, Some(&owner));
    assert_eq!(retry.rate_marks.len(), 1);
    read.compose_owner("directive", &retry.lines, None, false, now)
        .unwrap();
    apply_digest_rate_marks(svc, &retry.rate_marks);
    assert!(!svc
        .plan_turn_counter
        .digest_rule_due(&owner, "pressure", now_epoch_secs(), 3600));
}
