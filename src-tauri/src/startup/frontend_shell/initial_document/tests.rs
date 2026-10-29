use super::*;
use std::future::{poll_fn, Future};
use std::task::Poll;

#[test]
fn starting_window_is_hidden_and_opaque_even_when_main_is_transparent() {
    let mut config = tauri::utils::config::Config::default();
    config.app.windows.push(tauri::utils::config::WindowConfig {
        visible: true,
        transparent: true,
        ..Default::default()
    });
    let window = starting_window_config(&config).unwrap();
    assert_eq!(window.label, "starting");
    assert!(!window.visible);
    assert!(!window.transparent);
    assert_eq!(
        window.url,
        WebviewUrl::CustomProtocol(starting_url().unwrap())
    );
}

#[test]
fn starting_paint_requires_the_current_document_acknowledgement_once() {
    let ack = |generation| {
        Url::parse(&format!("domios-starting-painted://localhost/{generation}")).unwrap()
    };
    let mut paint = StartingPaint::default();
    assert!(!paint.acknowledge(&ack(0)));
    paint.begin();
    let old = paint.generation();
    paint.begin();
    assert!(!paint.acknowledge(&ack(old)));
    for invalid in [
        "domios-shell://localhost/2",
        "domios-starting-painted://foreign/2",
        "domios-starting-painted://localhost/2?stale=1",
        "domios-starting-painted://localhost/2#stale",
        "domios-starting-painted://user@localhost/2",
    ] {
        assert!(!paint.acknowledge(&Url::parse(invalid).unwrap()));
    }
    assert!(paint.acknowledge(&ack(paint.generation())));
    assert!(!paint.acknowledge(&ack(paint.generation())));
    paint.begin();
    assert!(paint.acknowledge(&ack(paint.generation())));
}

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
