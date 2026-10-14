use super::*;

#[test]
fn listener_restart_contract_reserves_only_the_next_health_proven_generation() {
    assert_eq!(reserve(8, &RestApiPhase::Listening, None).unwrap(), 9);
    assert!(reserve(8, &RestApiPhase::Listening, Some(9)).is_err());
    assert!(reserve(u64::MAX, &RestApiPhase::Listening, None).is_err());
    for phase in [RestApiPhase::Starting, RestApiPhase::Failed] {
        assert!(reserve(8, &phase, None).is_err());
    }
}

#[tokio::test]
async fn listener_restart_contract_supervisor_waits_for_response_completion() {
    let control = RestApiControl::new();
    let generation = control.begin_generation();
    control.listener_bound(generation, "127.0.0.1:12345".parse().unwrap());
    control.mark_listening(generation, vec![]).unwrap();
    let (commands, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    control.install(commands);
    let (accepted, acceptance) = oneshot::channel();
    let (body, body_finished) = oneshot::channel();
    accept(&control, accepted, body_finished);
    assert_eq!(acceptance.await.unwrap().unwrap(), 2);
    assert!(receiver.try_recv().is_err());
    body.send(()).unwrap();
    let command = receiver.recv().await.unwrap();
    let RestApiCommand::ReservedRestart {
        generation,
        completed,
    } = command
    else {
        panic!("unexpected command");
    };
    assert_eq!(generation, 2);
    assert_eq!(control.status().generation, 1);
    completed.send(control.status()).unwrap();
}

#[tokio::test]
async fn listener_restart_contract_deadline_only_classifies_failure() {
    let pending = std::future::pending::<Result<(), std::io::Error>>();
    assert!(
        super::super::drain_server(pending, std::time::Duration::ZERO)
            .await
            .is_err()
    );
    let complete = std::future::ready(Ok(()));
    assert!(
        super::super::drain_server(complete, std::time::Duration::ZERO)
            .await
            .is_ok()
    );
}
