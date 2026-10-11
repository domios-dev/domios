use super::*;

#[test]
fn frontend_shell_contract_release_ignores_development_url() {
    for target in ["http://127.0.0.1:1421", "https://foreign.example"] {
        assert_eq!(
            development_frontend(false, Some(&Url::parse(target).unwrap())).unwrap(),
            None
        );
    }
}

#[test]
fn frontend_shell_contract_debug_selects_configured_loopback_development_url() {
    assert_eq!(development_frontend(true, None).unwrap(), None);
    for target in [
        "http://127.0.0.1:1421/",
        "http://localhost:1422/",
        "http://[::1]:1423/",
    ] {
        let url = Url::parse(target).unwrap();
        assert_eq!(development_frontend(true, Some(&url)).unwrap(), Some(url));
    }
}

#[test]
fn frontend_shell_contract_refuses_untrusted_development_url() {
    for target in [
        "https://foreign.example",
        "http://192.168.1.2:1420",
        "http://user@127.0.0.1:1420",
        "file:///index.html",
    ] {
        assert!(development_frontend(true, Some(&Url::parse(target).unwrap())).is_err());
    }
}

fn boot() -> BootObservation {
    BootObservation {
        navigation: 3,
        listener_generation: 2,
        frontend_generation: 8,
        bundled_fallback: false,
    }
}

fn listener() -> RestApiStatus {
    RestApiStatus {
        generation: 2,
        phase: RestApiPhase::Listening,
        diagnostic: None,
        message: None,
    }
}

#[test]
fn frontend_shell_contract_boot_failure_selects_fallback_then_unavailable() {
    assert_eq!(
        boot_failure_action(boot(), 3, &listener(), 8),
        BootFailureAction::BundledFallback
    );
    assert_eq!(
        boot_failure_action(
            BootObservation {
                bundled_fallback: true,
                ..boot()
            },
            3,
            &listener(),
            8
        ),
        BootFailureAction::Unavailable
    );
}

#[test]
fn frontend_shell_contract_stale_boot_observers_cannot_replace_a_recovered_page() {
    assert_eq!(
        boot_failure_action(boot(), 4, &listener(), 8),
        BootFailureAction::Ignore
    );
    assert_eq!(
        boot_failure_action(boot(), 3, &listener(), 9),
        BootFailureAction::Ignore
    );
    assert_eq!(
        boot_failure_action(
            boot(),
            3,
            &RestApiStatus {
                generation: 3,
                ..listener()
            },
            8
        ),
        BootFailureAction::Ignore
    );
    for phase in [RestApiPhase::Starting, RestApiPhase::Failed] {
        assert_eq!(
            boot_failure_action(
                boot(),
                3,
                &RestApiStatus {
                    phase,
                    ..listener()
                },
                8
            ),
            BootFailureAction::Ignore
        );
    }
}
