use super::*;

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
