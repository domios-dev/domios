use super::*;
use codeterm_neighbors::keys::{KeyStore, Slot};

#[tokio::test]
async fn injected_keys_share_scope_and_slots_without_crossing_service_or_retired_binding() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let other = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let keys = fixture.service.fixture_neighbor_key_store();
    let other_keys = other.service.fixture_neighbor_key_store();
    let scope = KeyScope::installation("fixture-machine", "fixture-owner").unwrap();
    let foreign = KeyScope::installation("fixture-machine", "other-owner").unwrap();
    keys.save(&scope, Slot::Current, Zeroizing::new(vec![7; 64]))
        .await
        .unwrap();
    keys.save(&scope, Slot::Candidate, Zeroizing::new(vec![9; 64]))
        .await
        .unwrap();
    let selected = fixture.service.neighbor_key_store().unwrap();
    let activation = fixture.service.neighbor_key_store().unwrap();
    assert_eq!(
        selected
            .load(&scope, Slot::Current)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        &[7; 64]
    );
    assert_eq!(
        activation
            .load(&scope, Slot::Candidate)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        &[9; 64]
    );
    assert!(activation
        .load(&foreign, Slot::Current)
        .await
        .unwrap()
        .is_none());
    assert!(other_keys
        .load(&scope, Slot::Current)
        .await
        .unwrap()
        .is_none());
    selected.delete(&scope, Slot::Candidate).await.unwrap();
    assert!(activation
        .load(&scope, Slot::Candidate)
        .await
        .unwrap()
        .is_none());
    assert!(activation
        .load(&scope, Slot::Current)
        .await
        .unwrap()
        .is_some());
    drop(selected);
    drop(activation);
    drop(keys);
    assert!(matches!(
        fixture.service.neighbor_key_store(),
        Err(Error::SecretStore)
    ));
    assert!(other.service.neighbor_key_store().is_ok());
}
