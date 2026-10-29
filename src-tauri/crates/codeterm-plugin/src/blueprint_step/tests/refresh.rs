use super::*;

#[test]
fn plugin_steps_refresh_price_never_reserves_submits_or_auto_approves() {
    let request = request("refresh");
    let ledger = ledger(&request, 100);
    let descriptor = descriptor();
    let fixture = Fixture::new(5);
    *fixture.estimate.lock() = None;
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    assert_eq!(
        drive(&runtime, &request).waiting.unwrap().reason,
        WaitingReason::EstimateMissing
    );
    assert!(ledger
        .authorize_fresh("refresh", &request.context.scope, "owner", 5, 100)
        .is_err());
    *fixture.estimate.lock() = Some(Estimate {
        amount: 5,
        unit: request.unit.clone(),
        valid_until_ms: 200,
    });
    let refreshed = runtime
        .refresh_estimate(
            &request,
            || 100,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap();
    assert_eq!(refreshed.state, OperationState::WaitingForOwnerOk);
    assert_eq!(
        refreshed.waiting.unwrap().reason,
        WaitingReason::OwnerConfirmation
    );
    assert_eq!(refreshed.reserved, 0);
    assert!(refreshed.authorized.is_none());
    assert_eq!(
        drive(&runtime, &request).state,
        OperationState::WaitingForOwnerOk
    );
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 0);
    assert!(ledger
        .authorize_fresh("refresh", &request.context.scope, "owner", 5, 200)
        .is_err());
    ledger
        .authorize_fresh("refresh", &request.context.scope, "owner", 5, 100)
        .unwrap();
    assert_eq!(drive(&runtime, &request).state, OperationState::Queued);
    assert!(runtime
        .refresh_estimate(
            &request,
            || 100,
            Instant::now() + Duration::from_secs(5),
            Default::default()
        )
        .is_err());
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
}

#[test]
fn plugin_steps_refresh_clears_old_approval_and_unsafe_quote() {
    let request = request("refresh-unsafe");
    let ledger = ledger(&request, 1);
    let descriptor = descriptor();
    let fixture = Fixture::new(5);
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    drive(&runtime, &request);
    ledger
        .authorize_fresh("refresh-unsafe", &request.context.scope, "owner", 5, 100)
        .unwrap();
    *fixture.estimate.lock() = Some(Estimate {
        amount: u64::MAX,
        unit: request.unit.clone(),
        valid_until_ms: 200,
    });
    let held = runtime
        .refresh_estimate(
            &request,
            || 100,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap();
    assert!(held.authorized.is_none());
    assert!(held.estimate.is_none());
    assert_eq!(held.waiting.unwrap().estimate, None);
    assert_eq!(held.reserved, 0);
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 0);
}
