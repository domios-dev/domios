use super::{RespondPromptOutcome, ServiceContext};
use codeterm_types::pty::PromptIdentity;

impl ServiceContext {
    pub async fn respond_prompt_async(
        &self,
        tab: &str,
        value: &str,
        expected: Option<&PromptIdentity>,
    ) -> Result<RespondPromptOutcome, String> {
        if let Some(result) =
            crate::service::agent_runtime::prompt_answers::respond(self, tab, value, expected).await
        {
            return result;
        }
        let owner = self.clone();
        let tab = tab.to_string();
        let value = value.to_string();
        let expected = expected.cloned();
        crate::service::blocking::run(move || {
            owner.respond_prompt_inner(&tab, &value, expected.as_ref())
        })
        .await
    }
}
