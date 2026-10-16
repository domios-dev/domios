#[tokio::test(flavor = "current_thread")]
async fn inbound_delivery_wait_does_not_block_the_async_executor() {
    let (release, released) = std::sync::mpsc::channel();
    let (started, ready) = tokio::sync::oneshot::channel();
    let delivery = tokio::spawn(super::deliver_off_runtime(move || {
        let _ = started.send(());
        released.recv().map_err(|error| error.to_string())?;
        Ok(())
    }));
    tokio::time::timeout(std::time::Duration::from_secs(1), ready)
        .await
        .expect("blocking handoff started")
        .unwrap();
    assert!(!delivery.is_finished());
    release.send(()).unwrap();
    assert_eq!(delivery.await.unwrap(), Ok(()));
}

#[tokio::test(flavor = "current_thread")]
async fn inbound_delivery_failure_is_preserved() {
    let error = "opaque-delivery-failure";
    assert_eq!(
        super::deliver_off_runtime(move || Err(error.into())).await,
        Err(error.into())
    );
}
