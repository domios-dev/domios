use super::*;
use std::future::{poll_fn, Future};
use std::task::Poll;

#[test]
fn starting_document_url_is_absolute_and_stable_in_packaged_and_development_modes() {
    let dev = Url::parse("http://127.0.0.1:1479/").unwrap();
    for dev_url in [None, Some(dev)] {
        let mut config = tauri::utils::config::Config::default();
        config.build.dev_url = dev_url;
        config.app.windows.push(Default::default());
        let window = window_config(&config).unwrap();
        assert_eq!(
            window.url,
            WebviewUrl::CustomProtocol(starting_url().unwrap())
        );
        for http_protocol in [false, true] {
            let starting = url(http_protocol).unwrap();
            assert!(!starting.cannot_be_a_base());
            assert_eq!(Url::parse(starting.as_str()).unwrap(), starting);
            assert_eq!(starting.path(), "/starting");
            assert_eq!(starting.query(), None);
            assert_eq!(starting.fragment(), None);
            assert_eq!(
                starting.as_str(),
                if http_protocol {
                    "http://domios-startup.localhost/starting"
                } else {
                    "domios-startup://localhost/starting"
                }
            );
        }
    }
}
#[test]
fn starting_protocol_serves_the_branded_document_as_html() {
    let response = protocol_response();
    assert_eq!(response.status(), tauri::http::StatusCode::OK);
    assert_eq!(
        response.headers()[tauri::http::header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(
        response.body(),
        brand_fallback_page(BrandFallback::Starting).as_bytes()
    );
}

#[tokio::test]
async fn missing_first_document_acknowledgement_becomes_failure_never_ready() {
    let initial = InitialDocument::default();
    initial.begin(1);
    let (expired, deadline) = tokio::sync::oneshot::channel();
    let mut wait = Box::pin(initial.observe_failure(1, async {
        deadline.await.unwrap();
        Err(())
    }));
    assert!(poll_fn(|cx| Poll::Ready(wait.as_mut().poll(cx)))
        .await
        .is_pending());
    assert_eq!(*initial.0.borrow(), Phase::Awaiting(1));
    expired.send(()).unwrap();
    assert!(wait.await);
    assert!(initial.failed(1));
    assert!(!initial.acknowledge(1));
    assert_eq!(*initial.0.borrow(), Phase::Failed(1));
}

#[tokio::test]
async fn late_acknowledgement_or_failure_cannot_replace_a_newer_surface() {
    let initial = InitialDocument::default();
    initial.begin(1);
    assert!(initial.transition(1, Phase::Failed(1)));
    initial.begin(2);
    assert!(!initial.acknowledge(1));
    assert!(
        !initial
            .observe_failure(1, std::future::ready(Err(())))
            .await
    );
    assert_eq!(*initial.0.borrow(), Phase::Awaiting(2));
    assert!(initial.acknowledge(2));
    assert!(
        !initial
            .observe_failure(2, std::future::ready(Err(())))
            .await
    );
    initial.retire();
    assert!(!initial.acknowledge(2));
    assert_eq!(*initial.0.borrow(), Phase::Absent);
}
