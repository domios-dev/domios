use super::refresh_required;
use serde_json::json;

#[test]
fn admission_refreshes_signed_access_without_using_the_claim_as_a_grant() {
    for outcome in ["granted", "already_granted"] {
        assert_eq!(
            refresh_required(&json!({"schema_version":1,"outcome":outcome,
            "trial_end":1700604800,"friend_request_id":null})),
            Ok(true)
        );
    }
    for outcome in [
        "not_invited",
        "inviter_not_paid",
        "already_paid",
        "not_fresh",
        "invite_expired",
    ] {
        assert_eq!(
            refresh_required(&json!({"schema_version":1,"outcome":outcome,
            "trial_end":null,"friend_request_id":null})),
            Ok(false)
        );
    }
}

#[test]
fn malformed_claims_never_request_an_access_refresh() {
    for value in [
        json!({"schema_version":2,"outcome":"granted","trial_end":1700604800,"friend_request_id":null}),
        json!({"schema_version":1,"outcome":"granted","trial_end":null,"friend_request_id":null}),
        json!({"schema_version":1,"outcome":"not_invited","trial_end":1700604800,"friend_request_id":null}),
        json!({"schema_version":1,"outcome":"granted","trial_end":1700604800,"friend_request_id":"foreign"}),
        json!({"schema_version":1,"outcome":"unknown","trial_end":null,"friend_request_id":null}),
    ] {
        assert!(refresh_required(&value).is_err());
    }
}


#[tokio::test]
async fn claim_receipt_is_once_per_account_per_installation_across_restarts() {
    let installation = tempfile::tempdir().unwrap();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    for outcome in ["granted", "already_granted", "not_invited", "inviter_not_paid", "already_paid", "not_fresh", "invite_expired"] {
        let receipt = json!({"schema_version":1,"outcome":outcome,
            "trial_end":if matches!(outcome, "granted" | "already_granted") { Some(1700604800u64) } else { None },"friend_request_id":null});
        for _restart in 0..3 {
            let metadata = super::super::storage::Metadata::new(installation.path().to_owned(), outcome);
            let result = super::claim_once(&metadata, async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(receipt.clone())
            }).await.unwrap();
            assert_eq!(result, receipt);
        }
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 7);
    let other_installation = tempfile::tempdir().unwrap();
    let metadata = super::super::storage::Metadata::new(other_installation.path().to_owned(), "granted");
    super::claim_once(&metadata, async {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(super::no_claim())
    }).await.unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 8);
}

#[tokio::test]
async fn uncertain_claim_is_reserved_before_the_call_and_never_replayed() {
    let installation = tempfile::tempdir().unwrap();
    let metadata = super::super::storage::Metadata::new(installation.path().to_owned(), "owner");
    assert_eq!(super::claim_once(&metadata, async { Err(codeterm_neighbors::Error::Transport) }).await,
        Err(codeterm_neighbors::Error::Transport));
    let restarted = super::super::storage::Metadata::new(installation.path().to_owned(), "owner");
    assert_eq!(super::claim_once(&restarted, async { panic!("A written claim must not replay") }).await,
        Ok(super::no_claim()));
}
