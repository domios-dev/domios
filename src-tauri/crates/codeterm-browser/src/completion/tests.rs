use super::*;
use std::sync::Arc;
use tokio::sync::mpsc;

fn observation() -> (ActionCompletion, mpsc::Sender<Arc<CdpEvent>>) {
    let (sender, receiver) = mpsc::channel(64);
    (
        ActionCompletion {
            events: EventStream::from_receiver(receiver),
            frame: "main".into(),
            previous_loader: "before".into(),
            session: "own".into(),
            target: "target".into(),
            requested: false,
            loader: None,
            loaded: HashSet::new(),
            url: None,
            same_document: false,
            restored: false,
        },
        sender,
    )
}

fn event(method: &str, params: Value) -> Arc<CdpEvent> {
    Arc::new(CdpEvent {
        method: method.into(),
        params,
        session_id: Some("own".into()),
    })
}

#[tokio::test]
async fn completion_withheld_navigation_needs_its_main_frame_loader_load() {
    let (observation, sender) = observation();
    let finish = observation.finish(
        async {
            sender
                .send(event(
                    "Page.frameRequestedNavigation",
                    json!({"frameId":"main"}),
                ))
                .await
                .unwrap();
            Ok(Value::Null)
        },
        false,
        Duration::from_secs(15),
    );
    tokio::pin!(finish);
    assert!(futures_util::poll!(&mut finish).is_pending());
    sender
        .send(event(
            "Page.frameNavigated",
            json!({"frame":{"id":"main","loaderId":"new","url":"https://next/"}}),
        ))
        .await
        .unwrap();
    for (frame, loader) in [("child", "new"), ("main", "before")] {
        sender
            .send(event(
                "Page.lifecycleEvent",
                json!({"frameId":frame,"loaderId":loader,"name":"load"}),
            ))
            .await
            .unwrap();
    }
    let mut foreign = (*event(
        "Page.lifecycleEvent",
        json!({"frameId":"main","loaderId":"new","name":"load"}),
    ))
    .clone();
    foreign.session_id = Some("foreign".into());
    sender.send(Arc::new(foreign)).await.unwrap();
    assert!(futures_util::poll!(&mut finish).is_pending());
    sender
        .send(event(
            "Page.lifecycleEvent",
            json!({"frameId":"main","loaderId":"new","name":"load"}),
        ))
        .await
        .unwrap();
    assert_eq!(finish.await.unwrap().1, "navigation loaded: https://next/");
}

#[tokio::test]
async fn completion_same_document_and_non_navigation_have_positive_boundaries() {
    let (observation, sender) = observation();
    let (_, receipt) = observation
        .finish(
            async {
                sender
                    .send(event(
                        "Page.navigatedWithinDocument",
                        json!({"frameId":"main","url":"https://page/#new"}),
                    ))
                    .await
                    .unwrap();
                Ok(json!({"frameId":"main"}))
            },
            true,
            Duration::from_secs(15),
        )
        .await
        .unwrap();
    assert_eq!(receipt, "same-document navigation: https://page/#new");
    let (observation, _sender) = self::observation();
    let (_, receipt) = observation
        .finish(
            async { Ok(json!({"result":{"value":"performed"}})) },
            false,
            Duration::from_secs(15),
        )
        .await
        .unwrap();
    assert_eq!(
        receipt,
        "action acknowledged; asynchronous effects require an explicit state wait"
    );
}

#[tokio::test]
async fn completion_closed_page_and_deadline_never_report_settlement() {
    for method in [
        "Inspector.detached",
        "Target.targetDestroyed",
        "Target.detachedFromTarget",
    ] {
        let (observation, sender) = observation();
        let result = observation
            .finish(
                async {
                    sender
                        .send(event(
                            method,
                            json!({"targetId":"target","sessionId":"own"}),
                        ))
                        .await
                        .unwrap();
                    Ok(Value::Null)
                },
                false,
                Duration::from_secs(15),
            )
            .await;
        assert!(result.unwrap_err().contains("page closed"));
    }
    let (observation, _sender) = observation();
    assert!(observation
        .finish(std::future::pending(), true, Duration::ZERO)
        .await
        .unwrap_err()
        .contains("deadline exceeded"));
    let (observation, sender) = self::observation();
    drop(sender);
    assert!(observation
        .finish(async { Ok(Value::Null) }, false, Duration::from_secs(15))
        .await
        .unwrap_err()
        .contains("observation closed"));
}

#[tokio::test]
async fn completion_rejects_superseded_navigation_and_accepts_cache_restore() {
    let (observation, sender) = observation();
    let result = observation.finish(async {
        sender.send(event("Page.frameNavigated", json!({"frame":{"id":"main","loaderId":"unrelated","url":"https://other/"}}))).await.unwrap();
        Ok(json!({"loaderId":"own-loader"}))
    }, true, Duration::from_secs(15)).await;
    assert!(result.unwrap_err().contains("superseded"));
    let (observation, sender) = self::observation();
    let result = observation.finish(async {
        sender.send(event("Page.frameNavigated", json!({"frame":{"id":"main","loaderId":"restored","url":"https://back/"},"type":"BackForwardCacheRestore"}))).await.unwrap();
        Ok(Value::Null)
    }, true, Duration::from_secs(15)).await.unwrap();
    assert_eq!(result.1, "navigation restored: https://back/");
}

#[tokio::test]
async fn completion_same_document_event_cannot_settle_another_pending_full_navigation() {
    let (observation, sender) = observation();
    let action = observation.finish(
        async {
            sender
                .send(event(
                    "Page.navigatedWithinDocument",
                    json!({"frameId":"main","url":"https://page/#first"}),
                ))
                .await
                .unwrap();
            sender
                .send(event(
                    "Page.frameRequestedNavigation",
                    json!({"frameId":"main"}),
                ))
                .await
                .unwrap();
            Ok(Value::Null)
        },
        false,
        Duration::from_secs(15),
    );
    tokio::pin!(action);
    assert!(futures_util::poll!(&mut action).is_pending());
    sender
        .send(event(
            "Page.frameNavigated",
            json!({"frame":{"id":"main","loaderId":"new","url":"https://page/full"}}),
        ))
        .await
        .unwrap();
    sender
        .send(event(
            "Page.lifecycleEvent",
            json!({"frameId":"main","loaderId":"new","name":"load"}),
        ))
        .await
        .unwrap();
    assert_eq!(
        action.await.unwrap().1,
        "navigation loaded: https://page/full"
    );

    let (observation, sender) = self::observation();
    let result = observation
        .finish(
            async {
                sender
                    .send(event(
                        "Page.frameRequestedNavigation",
                        json!({"frameId":"main"}),
                    ))
                    .await
                    .unwrap();
                sender
                    .send(event(
                        "Page.frameStartedNavigating",
                        json!({"frameId":"main","navigationType":"sameDocument"}),
                    ))
                    .await
                    .unwrap();
                sender
                    .send(event(
                        "Page.navigatedWithinDocument",
                        json!({"frameId":"main","url":"https://page/#hash"}),
                    ))
                    .await
                    .unwrap();
                Ok(Value::Null)
            },
            false,
            Duration::from_secs(15),
        )
        .await
        .unwrap();
    assert_eq!(result.1, "same-document navigation: https://page/#hash");
}
