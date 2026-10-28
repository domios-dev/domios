use super::*;

#[tokio::test]
async fn ack_commit_persists_proof_and_keeps_pending_visibility_while_neighbors_is_off() {
    let (mux,_) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.ext::<crate::config::ConfigState>().lock().neighbors.enabled = false;
    let metadata = Metadata::new(fixture.temp.path().to_owned(),"owner");
    let friendship = "a".repeat(64);
    let message = "b".repeat(32);
    metadata.write(format!("history-{friendship}.json"),json!({"ended":false,"read_requested_through":message,"entries":[{"direction":"in","author":"friend","sender_uid":"peer","body":"opaque","at":1,"message_id":message,"kind":"question"}]})).await.unwrap();
    let generation = fixture.service.neighbor_history_generation();
    fixture.service.commit_neighbor_incoming_ack(&metadata,&friendship,&message,generation).await.unwrap();
    let mut saved = metadata.history(&friendship).await.unwrap();
    assert_eq!(saved.received_acknowledged_through,Some(message.clone()));
    assert_eq!(saved.pending_read_through(),Some(message.clone()));
    assert!(saved.read_receipt_through.is_none());
    assert!(!saved.reserve_read_through(&message,false,false));
    assert!(saved.reserve_read_through(&message,true,false));
}
