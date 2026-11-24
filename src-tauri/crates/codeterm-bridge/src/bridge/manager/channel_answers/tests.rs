#[tokio::test(flavor = "current_thread")]
async fn choice_can_enter_the_runtime_once_from_the_blocking_claim_port() {
    let runtime = tokio::runtime::Handle::current();
    let result = super::claim_off_runtime(move || runtime.block_on(async { Some(Ok(())) })).await;
    assert_eq!(result, Some(Ok(())));
}

#[tokio::test]
async fn an_interrupted_claim_is_consumed_and_cannot_fall_through_to_text_delivery() {
    let result = super::claim_off_runtime(|| panic!("opaque interruption")).await;
    assert!(matches!(result, Some(Err(_))));
}
