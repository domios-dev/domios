use serde_json::Value;

use super::handlers::{Effect, Status};
use super::machine::Route;
use super::probe::Output;
use super::source::TabBinding;
use super::store::Sentinel;
use crate::service::supporter_tab_state::sentinel_request::RequestEffect;
use crate::service::ServiceContext;

pub(super) fn respond_prompt(
    service: &ServiceContext,
    job: &Sentinel,
    route: &Route,
    output: &Output,
) -> Result<Effect, String> {
    let Route::RespondPrompt(kind) = route else {
        return Err("sentinel respond_prompt route mismatch".into());
    };
    let tab_id = match job.observe.as_ref() {
        Some(TabBinding::Tab(tab_id)) => tab_id.as_str(),
        Some(TabBinding::CronRun(_)) => job
            .run
            .as_ref()
            .map(|run| run.tab_id.as_str())
            .ok_or("sentinel cron run binding is unavailable")?,
        None => return Err("respond_prompt requires a tab observation source".into()),
    };
    let observed: Value =
        serde_json::from_str(&output.text).map_err(|_| "sentinel prompt observation is invalid")?;
    let prompt = observed
        .get("prompt")
        .filter(|prompt| !prompt.is_null())
        .ok_or("sentinel prompt observation has no prompt identity")?;
    match service.request_safe_prompt_response(tab_id, kind, prompt, &job.id) {
        Ok(RequestEffect::Completed(observation_id)) => Ok(Effect {
            status: Status::Completed,
            execution_id: Some(observation_id.to_string()),
            command_result: None,
        }),
        Ok(RequestEffect::AlreadyClaimed) => Ok(Status::Noop.into()),
        Err(refusal) => Err(refusal.reason().into()),
    }
}
