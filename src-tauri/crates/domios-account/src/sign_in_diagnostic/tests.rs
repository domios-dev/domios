use super::*;

#[tokio::test]
async fn injected_failure_is_attributed_to_the_operation_being_awaited() {
    for stage in [
        SignInStage::Preparation,
        SignInStage::Browser,
        SignInStage::Callback,
        SignInStage::Redemption,
        SignInStage::CredentialSave,
        SignInStage::Publication,
    ] {
        let mut attempt = SignInAttempt::default();
        let result = attempt
            .run(stage, async { Err::<(), _>(Error::Transport) })
            .await;
        assert_eq!(
            attempt.failure(result.err()),
            Some(SignInFailure {
                stage,
                error: Error::Transport
            })
        );
        let encoded = serde_json::to_value(attempt.failure(Some(Error::Provider))).unwrap();
        assert_eq!(encoded.as_object().unwrap().len(), 2);
        assert_eq!(encoded["error"], "provider");
        assert!(encoded["stage"].is_string());
        assert!(attempt.failure(None).is_none());
    }
}

#[test]
fn stages_and_errors_have_stable_redacted_log_names() {
    assert_eq!(SignInStage::Redemption.name(), "redemption");
    assert_eq!(SignInStage::CredentialSave.name(), "credential_save");
    assert_eq!(error_kind(Error::Provider), "provider");
    assert_eq!(error_kind(Error::Credentials), "credentials");
    assert_eq!(error_kind(Error::WrongState), "wrong_state");
}
