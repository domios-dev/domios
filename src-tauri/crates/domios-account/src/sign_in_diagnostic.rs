use crate::{Error, Result};
use serde::Serialize;
use std::future::Future;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SignInStage {
    #[default]
    Preparation,
    Browser,
    Callback,
    Redemption,
    CredentialSave,
    Publication,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SignInFailure {
    pub stage: SignInStage,
    pub error: Error,
}
#[derive(Default)]
pub(crate) struct SignInAttempt {
    stage: SignInStage,
}
impl SignInAttempt {
    pub fn enter(&mut self, stage: SignInStage) {
        self.stage = stage;
    }
    pub async fn run<T>(
        &mut self,
        stage: SignInStage,
        operation: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        self.enter(stage);
        operation.await
    }
    pub fn failure(&self, error: Option<Error>) -> Option<SignInFailure> {
        error.map(|error| SignInFailure {
            stage: self.stage,
            error,
        })
    }
}

#[cfg(test)]
mod tests {
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
}
