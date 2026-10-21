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
impl SignInStage {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Preparation => "preparation",
            Self::Browser => "browser",
            Self::Callback => "callback",
            Self::Redemption => "redemption",
            Self::CredentialSave => "credential_save",
            Self::Publication => "publication",
        }
    }
}

pub(crate) const fn error_kind(error: Error) -> &'static str {
    match error {
        Error::Configuration => "configuration",
        Error::InvalidCallback => "invalid_callback",
        Error::WrongState => "wrong_state",
        Error::Consumed => "consumed",
        Error::Timeout => "timeout",
        Error::Cancelled => "cancelled",
        Error::Pending => "pending",
        Error::Transport => "transport",
        Error::Provider => "provider",
        Error::Credentials => "credentials",
        Error::Browser => "browser",
    }
}

pub(crate) fn log_callback_rejection(error: Error) {
    log::info!(target: "codeterm_account::sign_in", "sign_in callback outcome=rejected stage={} error_kind={}", SignInStage::Callback.name(), error_kind(error));
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
        log::info!(target: "codeterm_account::sign_in", "sign_in stage_transition stage={} error_kind=none", stage.name());
    }
    pub async fn run<T>(
        &mut self,
        stage: SignInStage,
        operation: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        self.enter(stage);
        match operation.await {
            Ok(value) => {
                log::info!(target: "codeterm_account::sign_in", "sign_in stage_outcome stage={} outcome=success error_kind=none", stage.name());
                Ok(value)
            }
            Err(error) => {
                log::info!(target: "codeterm_account::sign_in", "sign_in stage_outcome stage={} outcome=failure error_kind={}", stage.name(), error_kind(error));
                Err(error)
            }
        }
    }
    pub fn failure(&self, error: Option<Error>) -> Option<SignInFailure> {
        error.map(|error| SignInFailure {
            stage: self.stage,
            error,
        })
    }
}

#[cfg(test)]
#[path = "sign_in_diagnostic/tests.rs"]
mod tests;
