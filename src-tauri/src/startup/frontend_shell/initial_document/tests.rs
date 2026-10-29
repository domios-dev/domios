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
fn starting_layout_requires_the_current_document_acknowledgement_once() {
    let ack = |generation| {
        Url::parse(&format!("domios-starting-ready://localhost/{generation}")).unwrap()
    };
    let layout = StartingLayout::default();
    assert!(!layout.acknowledge(&ack(0)));
    layout.begin();
    let old = layout.generation();
    layout.begin();
    assert!(!layout.acknowledge(&ack(old)));
    for invalid in [
        "domios-shell://localhost/2",
        "domios-starting-ready://foreign/2",
        "domios-starting-ready://localhost/2?stale=1",
        "domios-starting-ready://localhost/2#stale",
        "domios-starting-ready://user@localhost/2",
    ] {
        assert!(!layout.acknowledge(&Url::parse(invalid).unwrap()));
    }
    assert!(layout.acknowledge(&ack(layout.generation())));
    assert!(!layout.acknowledge(&ack(layout.generation())));
    layout.begin();
    assert!(layout.acknowledge(&ack(layout.generation())));
}

#[test]
fn unavailable_starting_layout_is_not_ready_and_only_current_failure_can_open_recovery() {
    let failure = |generation| {
        Url::parse(&format!(
            "domios-starting-unavailable://localhost/{generation}"
        ))
        .unwrap()
    };
    let layout = StartingLayout::default();
    assert!(layout.refuse(&failure(0)).is_none());
    layout.begin();
    let old = layout.generation();
    layout.begin();
    assert!(layout.refuse(&failure(old)).is_none());
    assert!(!layout.acknowledge(&failure(layout.generation())));
    assert!(layout.refuse(&failure(layout.generation())).is_some());
    assert!(layout.refuse(&failure(layout.generation())).is_none());
    let ready = Url::parse(&format!(
        "domios-starting-ready://localhost/{}",
        layout.generation()
    ))
    .unwrap();
    assert!(!layout.acknowledge(&ready));
    layout.begin();
    assert!(layout.refuse(&failure(old)).is_none());
}

#[test]
fn starting_eval_and_reveal_refusals_select_native_recovery_once() {
    for reveal_refused in [false, true] {
        let layout = StartingLayout::default();
        let generation = layout.begin();
        let ready = Url::parse(&format!("domios-starting-ready://localhost/{generation}")).unwrap();
        let called = std::cell::Cell::new(0);
        let refusal = || {
            called.set(called.get() + 1);
            Err::<(), _>("fixture refusal")
        };
        let fallback = if reveal_refused {
            layout.reveal(&ready, refusal) == Some(generation)
        } else {
            layout.request(generation, refusal)
        };
        assert!(fallback);
        assert_eq!(called.get(), 1);
        assert!(layout.claim_recovery(generation));
        assert!(!layout.claim_recovery(generation));
        assert!(layout.recovery_active(generation));
        assert!(!layout.request(generation, || -> Result<(), ()> {
            panic!("A resolved document must not evaluate again")
        }));
        assert!(layout
            .reveal(&ready, || -> Result<(), ()> {
                panic!("A failed document must not reveal")
            })
            .is_none());
        layout.begin();
        assert!(!layout.recovery_active(generation));
        assert!(!layout.claim_recovery(generation));
    }
}

#[tokio::test]
async fn starting_missing_acknowledgement_deadline_selects_failure_never_readiness() {
    let layout = StartingLayout::default();
    let generation = layout.begin();
    let (expire, deadline) = tokio::sync::oneshot::channel();
    let mut failure = Box::pin(layout.observe_failure(generation, async {
        deadline.await.unwrap();
        Err(())
    }));
    assert!(poll_fn(|cx| Poll::Ready(failure.as_mut().poll(cx)))
        .await
        .is_pending());
    assert!(layout.awaiting(generation));
    assert!(!layout.claim_recovery(generation));
    expire.send(()).unwrap();
    assert!(failure.await);
    assert!(!layout.awaiting(generation));
    assert!(layout.claim_recovery(generation));
    assert!(!layout.claim_recovery(generation));
    assert!(
        !layout
            .observe_failure(generation, std::future::ready(Err(())))
            .await
    );
    let ready = Url::parse(&format!("domios-starting-ready://localhost/{generation}")).unwrap();
    assert!(layout
        .reveal(&ready, || -> Result<(), ()> {
            panic!("Expiry cannot reveal the window")
        })
        .is_none());
}

#[tokio::test]
async fn starting_success_replacement_and_destruction_resolve_the_failure_wait() {
    for resolution in ["ready", "replaced", "destroyed"] {
        let layout = StartingLayout::default();
        let generation = layout.begin();
        let mut wait = Box::pin(layout.resolved(generation));
        assert!(poll_fn(|cx| Poll::Ready(wait.as_mut().poll(cx)))
            .await
            .is_pending());
        match resolution {
            "ready" => {
                let ready =
                    Url::parse(&format!("domios-starting-ready://localhost/{generation}")).unwrap();
                assert!(layout.reveal(&ready, || Ok::<(), ()>(())).is_none());
                assert!(!layout.awaiting(generation));
            }
            "replaced" => {
                layout.begin();
            }
            "destroyed" => layout.retire(),
            _ => unreachable!(),
        }
        wait.await;
        assert!(
            !layout
                .observe_failure(generation, std::future::ready(Err(())))
                .await
        );
        assert!(!layout.claim_recovery(generation));
    }
}

#[test]
fn starting_port_failures_cannot_select_recovery_for_a_replaced_generation() {
    let layout = StartingLayout::default();
    let generation = layout.begin();
    assert!(!layout.request(generation, || {
        layout.begin();
        Err::<(), _>("old evaluation refused")
    }));
    assert!(!layout.claim_recovery(generation));
    assert!(!layout.request(generation, || -> Result<(), ()> {
        panic!("An old generation must not request layout")
    }));
    let ready = Url::parse(&format!(
        "domios-starting-ready://localhost/{}",
        layout.generation()
    ))
    .unwrap();
    let replaced = layout.generation();
    assert!(layout
        .reveal(&ready, || {
            layout.begin();
            Err::<(), _>("old reveal refused")
        })
        .is_none());
    assert!(!layout.claim_recovery(replaced));
    assert!(layout.awaiting(layout.generation()));
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
