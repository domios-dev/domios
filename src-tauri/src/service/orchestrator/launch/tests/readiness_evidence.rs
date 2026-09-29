use super::{provider_ready_stage, BootStage};

#[test]
fn provider_ready_requires_authenticated_launch_evidence() {
    use crate::service::launch_readiness::LaunchSettlement;

    let ready = LaunchSettlement::Ready;
    let provisional = LaunchSettlement::Provisional {
        stage: BootStage::LaunchDeadlineElapsed,
        reason: "opaque-timeout-reason".into(),
    };
    assert_eq!(provider_ready_stage(&ready, false), None);
    assert_eq!(provider_ready_stage(&provisional, false), None);
    assert_eq!(
        provider_ready_stage(&ready, true),
        Some(BootStage::ProviderReady)
    );
    assert_eq!(
        provider_ready_stage(&provisional, true),
        Some(BootStage::ProviderReady)
    );

    let source = include_str!("../../launch.rs");
    let binding_wait = source
        .find("readiness_receiver.recv()")
        .expect("the provider binding settles before readiness is emitted");
    let provider_ready = source
        .find("provider_ready_stage(&readiness, session_authenticated)")
        .expect("ProviderReady follows the binding observation");
    assert!(binding_wait < provider_ready);
}
