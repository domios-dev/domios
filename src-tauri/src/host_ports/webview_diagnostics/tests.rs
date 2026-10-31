use super::*;

#[test]
fn main_target_requires_exact_document_and_view_navigation_pair() {
    let generation = ClientViewGeneration {
        view: 8,
        navigation: 21,
    };
    let main = with_root_generation(
        &Url::parse("http://127.0.0.1:7686/index.html?theme=dark").unwrap(),
        generation,
    );
    let same_document = Url::parse("http://127.0.0.1:7686/index.html?theme=dark").unwrap();
    let stale_navigation = with_root_generation(
        &Url::parse("http://127.0.0.1:7686/index.html?theme=dark").unwrap(),
        ClientViewGeneration {
            navigation: generation.navigation + 1,
            ..generation
        },
    );
    assert_eq!(root_generation_from_url(&main), Some(generation));
    assert_eq!(
        without_root_generation(&main),
        without_root_generation(&same_document)
    );
    assert_ne!(
        root_generation_from_url(&stale_navigation),
        Some(generation)
    );
    assert!(target_matches(
        "page",
        main.as_str(),
        "ws://127.0.0.1:9222/devtools/page/main",
        9222,
        generation,
        &main,
    ));
    assert!(!target_matches(
        "page",
        stale_navigation.as_str(),
        "ws://127.0.0.1:9222/devtools/page/main",
        9222,
        generation,
        &main,
    ));
    assert!(!target_matches(
        "page",
        main.as_str(),
        "ws://127.0.0.1:9223/devtools/page/main",
        9222,
        generation,
        &main,
    ));
}

#[test]
#[cfg(not(windows))]
fn requested_diagnostics_are_unsupported_off_windows() {
    for occupied in [true, false] {
        assert_eq!(
            initial_status(
                Some(9222),
                PortPreflight {
                    windows: false,
                    ..test_preflight(occupied)
                }
            ),
            NativeWebviewDiagnosticsStatus::Unsupported
        );
    }
    assert_eq!(
        initial_status(None, test_preflight(false)),
        NativeWebviewDiagnosticsStatus::Disabled
    );
}

#[test]
#[cfg(windows)]
fn unavailable_diagnostics_never_claims_a_foreign_or_unverified_listener() {
    assert!(matches!(
        initial_status(Some(9222), test_preflight(true)),
        NativeWebviewDiagnosticsStatus::Unavailable {
            reason: NativeWebviewDiagnosticsUnavailableReason::PortOccupied
        }
    ));
    assert!(matches!(
        initial_status(Some(9222), test_preflight(false)),
        NativeWebviewDiagnosticsStatus::Unavailable {
            reason: NativeWebviewDiagnosticsUnavailableReason::EndpointNotAttributed
        }
    ));
    assert_eq!(
        initial_status(None, test_preflight(false)),
        NativeWebviewDiagnosticsStatus::Disabled
    );
}

#[test]
fn diagnostics_listener_must_be_a_webview_process_owned_by_this_client() {
    let processes = [
        ProcessIdentity {
            pid: 10,
            parent_pid: 1,
            image: "domios-app.exe".into(),
        },
        ProcessIdentity {
            pid: 11,
            parent_pid: 10,
            image: "msedgewebview2.exe".into(),
        },
    ];
    assert!(is_owned_webview_listener(11, 10, &processes));
    assert!(!is_owned_webview_listener(99, 10, &processes));
    assert!(!is_owned_webview_listener(11, 2, &processes));
    let foreign_image = [ProcessIdentity {
        image: "other.exe".into(),
        ..processes[1].clone()
    }];
    assert!(!is_owned_webview_listener(11, 10, &foreign_image));
}

fn test_preflight(occupied: bool) -> PortPreflight {
    PortPreflight::evaluate(true, true, || {
        if occupied {
            Err(io::ErrorKind::AddrInUse.into())
        } else {
            Ok(())
        }
    })
}

#[test]
fn preflight_ineligible_build_or_platform_never_attempts_bind() {
    for (debug, windows) in [(false, true), (true, false), (false, false)] {
        let preflight = PortPreflight::evaluate(debug, windows, || panic!("ineligible bind"));
        assert_eq!(preflight.bind, BindOutcome::NotAttempted);
        assert!(!preflight.available());
        assert_eq!(
            initial_status(Some(9222), preflight),
            NativeWebviewDiagnosticsStatus::Unsupported
        );
    }
}

#[test]
fn preflight_available_is_not_attributed_or_active() {
    let preflight = PortPreflight::evaluate(true, true, || Ok(()));
    assert!(preflight.available());
    assert_eq!(
        initial_status(Some(9222), preflight),
        NativeWebviewDiagnosticsStatus::Unavailable {
            reason: NativeWebviewDiagnosticsUnavailableReason::EndpointNotAttributed,
        }
    );
}

#[test]
fn preflight_only_address_in_use_is_occupied() {
    for kind in [
        io::ErrorKind::AddrInUse,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::AddrNotAvailable,
        io::ErrorKind::Other,
    ] {
        let preflight = PortPreflight::evaluate(true, true, || Err(kind.into()));
        assert_eq!(
            preflight.bind,
            BindOutcome::Failed {
                kind,
                raw_os_error: None
            }
        );
        assert!(!preflight.available());
        let reason = if kind == io::ErrorKind::AddrInUse {
            NativeWebviewDiagnosticsUnavailableReason::PortOccupied
        } else {
            NativeWebviewDiagnosticsUnavailableReason::WebviewInitializationFailed
        };
        assert_eq!(
            initial_status(Some(9222), preflight),
            NativeWebviewDiagnosticsStatus::Unavailable { reason }
        );
    }
}

#[test]
fn preflight_preserves_raw_os_error_without_error_text() {
    let error = io::Error::from_raw_os_error(12345);
    let kind = error.kind();
    let preflight = PortPreflight::evaluate(true, true, || Err(error));
    assert_eq!(
        preflight.bind,
        BindOutcome::Failed {
            kind,
            raw_os_error: Some(12345)
        }
    );
}

#[test]
fn pending_or_mismatched_preflight_never_enables_inspection() {
    let session = DiagnosticsSession::new(crate::LocalWebviewDiagnosticsMode::RequestedPort(9222));
    session.set_port_preflight(9223, test_preflight(false));
    assert!(!session.preflight.lock().available());
    assert!(!matches!(
        session.bind_view(),
        NativeWebviewDiagnosticsStatus::Active { .. }
    ));
    session.set_port_preflight(9222, test_preflight(true));
    assert_eq!(
        session.bind_view(),
        NativeWebviewDiagnosticsStatus::Unavailable {
            reason: NativeWebviewDiagnosticsUnavailableReason::PortOccupied,
        }
    );
}

#[tokio::test]
async fn failed_or_pending_preflight_refuses_before_endpoint_inspection() {
    let generation = ClientViewGeneration {
        view: 8,
        navigation: 21,
    };
    let main = with_root_generation(
        &Url::parse("http://127.0.0.1:7686/index.html").unwrap(),
        generation,
    );
    for preflight in [
        PortPreflight::pending(true, true),
        test_preflight(true),
        PortPreflight::evaluate(true, true, || Err(io::ErrorKind::PermissionDenied.into())),
        PortPreflight::evaluate(false, true, || panic!("ineligible bind")),
    ] {
        assert_eq!(
            inspect_main_webview_diagnostics(9222, generation, main.clone(), preflight).await,
            initial_status(Some(9222), preflight)
        );
    }
}

#[test]
fn target_refuses_foreign_document_view_and_socket() {
    let generation = ClientViewGeneration {
        view: 8,
        navigation: 21,
    };
    let main = with_root_generation(
        &Url::parse("http://127.0.0.1:7686/index.html").unwrap(),
        generation,
    );
    let foreign_document = with_root_generation(
        &Url::parse("http://127.0.0.1:7686/foreign.html").unwrap(),
        generation,
    );
    let foreign_view = with_root_generation(
        &main,
        ClientViewGeneration {
            view: 9,
            ..generation
        },
    );
    for (target_type, document, socket) in [
        (
            "page",
            &foreign_document,
            "ws://127.0.0.1:9222/devtools/page/main",
        ),
        (
            "page",
            &foreign_view,
            "ws://127.0.0.1:9222/devtools/page/main",
        ),
        (
            "page",
            &main,
            "ws://foreign.invalid:9222/devtools/page/main",
        ),
        ("page", &main, "ws://127.0.0.1:9222/foreign/main"),
        ("worker", &main, "ws://127.0.0.1:9222/devtools/page/main"),
    ] {
        assert!(!target_matches(
            target_type,
            document.as_str(),
            socket,
            9222,
            generation,
            &main
        ));
    }
}

#[test]
#[cfg(debug_assertions)]
fn preflight_receipt_is_bounded_and_records_compiled_eligibility_and_phase() {
    let preflight =
        PortPreflight::evaluate(true, true, || Err(io::ErrorKind::PermissionDenied.into()));
    assert_eq!(
        preflight.receipt(Some(9222)),
        serde_json::json!({
            "requested_port": 9222,
            "compiled_debug": true,
            "compiled_windows": true,
            "phase": "before_starting_webview",
            "bind_outcome": "bind_failed",
            "bind_error_kind": "PermissionDenied",
            "raw_os_error": null,
            "failure_listener": null,
            "starting_view_present": null,
        })
    );
}

#[test]
#[cfg(debug_assertions)]
fn failure_listener_observation_requires_stable_pid_for_ancestry() {
    for (before, after, ancestry, expected) in [
        (Some(11), Some(11), true, Some(true)),
        (Some(11), Some(11), false, Some(false)),
        (Some(11), Some(12), true, None),
        (Some(11), None, true, None),
        (None, Some(11), true, None),
        (None, None, true, None),
    ] {
        let mut samples = [before, after].into_iter();
        let observation = FailureListenerObservation::capture(
            || samples.next().unwrap(),
            |pid| {
                assert_eq!(pid, 11);
                ancestry
            },
        );
        assert_eq!(
            observation,
            FailureListenerObservation {
                listener_pid_before: before,
                listener_pid_after: after,
                client_ancestry_verified: expected,
            }
        );
        assert_eq!(samples.next(), None);
    }
}

#[test]
#[cfg(debug_assertions)]
fn failure_observer_only_runs_for_eligible_address_in_use() {
    for preflight in [
        PortPreflight::pending(true, true),
        PortPreflight::pending(false, true),
        PortPreflight::pending(true, false),
        PortPreflight::evaluate(true, true, || Ok(())),
        PortPreflight::evaluate(true, true, || Err(io::ErrorKind::PermissionDenied.into())),
    ] {
        assert_eq!(
            preflight.observe_address_in_use(|| panic!("unrelated failure observer")),
            preflight
        );
    }
}

#[test]
#[cfg(debug_assertions)]
fn observed_client_ancestry_never_overrides_occupied_refusal() {
    let preflight = test_preflight(true)
        .observe_address_in_use(|| FailureListenerObservation::capture(|| Some(11), |_| true))
        .with_starting_view_present(true);
    assert!(!preflight.available());
    assert_eq!(
        initial_status(Some(9222), preflight),
        NativeWebviewDiagnosticsStatus::Unavailable {
            reason: NativeWebviewDiagnosticsUnavailableReason::PortOccupied,
        }
    );
    assert_eq!(
        preflight.receipt(Some(9222)),
        serde_json::json!({
            "requested_port": 9222,
            "compiled_debug": true,
            "compiled_windows": true,
            "phase": "before_starting_webview",
            "bind_outcome": "address_in_use",
            "bind_error_kind": "AddrInUse",
            "raw_os_error": null,
            "starting_view_present": true,
            "failure_listener": {
                "listener_pid_before": 11,
                "listener_pid_after": 11,
                "client_ancestry_verified": true,
            },
        })
    );
}

#[test]
fn starting_document_cannot_attest_main_target_even_with_current_generation() {
    let generation = ClientViewGeneration {
        view: 8,
        navigation: 21,
    };
    let main = with_root_generation(
        &Url::parse("http://127.0.0.1:15017/index.html").unwrap(),
        generation,
    );
    for starting in [
        "http://domios-startup.localhost/starting",
        "domios-startup://localhost/starting",
    ] {
        let starting = with_root_generation(&Url::parse(starting).unwrap(), generation);
        assert!(!target_matches(
            "page",
            starting.as_str(),
            "ws://127.0.0.1:9417/devtools/page/starting",
            9417,
            generation,
            &main,
        ));
    }
}
