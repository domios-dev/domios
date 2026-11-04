use super::{BrowserRuntime, WAIT_DEADLINE};
use codeterm_browser::{completion::ActionCompletion, PageChannel};
use serde_json::Value;
use std::future::Future;

impl BrowserRuntime {
    pub(super) async fn complete_action<F>(
        &self,
        page: &PageChannel,
        navigation_required: bool,
        action: F,
    ) -> Result<(Value, String), String>
    where
        F: Future<Output = Result<Value, String>>,
    {
        tokio::time::timeout(WAIT_DEADLINE, async {
            let authority = self.shared.begin_action(page)?;
            let observation = ActionCompletion::observe(page).await?;
            self.shared.check_action(&authority)?;
            let outcome = observation.finish(action, navigation_required, WAIT_DEADLINE).await?;
            self.shared.check_action(&authority)?;
            Ok(outcome)
        }).await.map_err(|_| "action_unconfirmed: completion deadline exceeded; inspect before continuing; never replay".to_string())?
    }
}

#[cfg(test)]
mod tests;
