use super::*;
use crate::sentinel::provenance::{DecidedBy, Decision};

#[test]
fn accepted_decision_survives_late_handler_completion_and_store_reload() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let job = create(
        &store,
        Some("team"),
        false,
        json!({"done":[
            {"id":"effect","type":"notify","parameters":{}}
        ]}),
    );
    let lease = due(&store).remove(0);
    run(&store, &lease, "ready", "done", |job, _, _| {
        commit(&store, |current| {
            current.stage_stop(&job.id, "external_stop")
        })?;
        Ok(Status::Completed)
    });
    let expected = Decision {
        decided_by: DecidedBy::Pattern,
        adapter_id: None,
        model: None,
        confidence: Some(1.0),
    };
    let persisted = stored(&storage, &job.id);
    assert!(persisted.record.runtime.stopped);
    assert_eq!(
        codec::decode(&persisted).transitions[0].decision,
        Some(expected.clone())
    );
    assert_eq!(
        codec::decode(&persisted).transitions[0].handlers[0].status,
        Status::Completed
    );
    drop(lease);
    drop(store);
    close(storage);
    let storage = open(root.path());
    assert_eq!(
        codec::decode(&stored(&storage, &job.id)).transitions[0].decision,
        Some(expected)
    );
    close(storage);
}

#[test]
fn accepted_adapter_identity_is_durable_separately_from_configuration() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let job = create(&store, Some("team"), false, json!({}));
    let lease = due(&store).remove(0);
    crate::sentinel::scheduler::advance(
        &store,
        &lease,
        &output("ambiguous"),
        || {
            let mut answer = Classification::adapter(Ok(Verdict {
                bucket: "done".into(),
                confidence: 0.9,
            }));
            answer.adapter_id = Some("used-adapter".into());
            answer.model = Some("used-model".into());
            answer
        },
        |job, transition, handler| {
            commit(&store, |current| {
                current.stage_claim_handler(job, transition, handler)
            })
        },
        |_, _, _, _| Ok(Status::Completed.into()),
        |_, _, _| Ok(()),
    );
    assert_eq!(
        codec::decode(&stored(&storage, &job.id)).transitions[0].decision,
        Some(Decision {
            decided_by: DecidedBy::Adapter,
            adapter_id: Some("used-adapter".into()),
            model: Some("used-model".into()),
            confidence: Some(0.9)
        })
    );
    close(storage);
}
