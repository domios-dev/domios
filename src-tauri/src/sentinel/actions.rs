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
        return Err("blueprint respond_prompt route mismatch".into());
    };
    let tab_id = match job.observe() {
        Some(TabBinding::Tab(tab_id)) => tab_id.as_str(),
        Some(TabBinding::CronRun(_)) => job
            .run
            .as_ref()
            .map(|run| run.tab_id.as_str())
            .ok_or("blueprint cron run binding is unavailable")?,
        None => return Err("respond_prompt requires a tab observation source".into()),
    };
    let observed: Value = match serde_json::from_str(&output.text) {
        Ok(observed) => observed,
        Err(_) => {
            return Ok(prompt_refused_effect(
                "blueprint_prompt_observation_invalid",
            ))
        }
    };
    let prompt = observed
        .get("prompt")
        .filter(|prompt| !prompt.is_null())
        .ok_or("blueprint_prompt_observation_missing_identity");
    let prompt = match prompt {
        Ok(prompt) => prompt,
        Err(reason) => return Ok(prompt_refused_effect(reason)),
    };
    match service.request_safe_prompt_response(tab_id, kind, prompt, &job.id) {
        Ok(effect) => Ok(prompt_request_effect(effect)),
        Err(refusal) => Ok(prompt_refused_effect(refusal.reason())),
    }
}

fn prompt_request_effect(effect: RequestEffect) -> Effect {
    let execution_id = match effect {
        RequestEffect::Held(observation_id) => observation_id.map(|id| id.to_string()),
        RequestEffect::AlreadyClaimed => None,
    };
    Effect {
        status: Status::Unconfirmed,
        execution_id,
        command_result: None,
        reason: Some("blueprint_prompt_answer_held".into()),
    }
}

fn prompt_refused_effect(reason: impl Into<String>) -> Effect {
    Effect {
        status: Status::Failed,
        execution_id: None,
        command_result: None,
        reason: Some(reason.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_observation_is_held_until_an_answer_is_confirmed() {
        for request in [
            RequestEffect::Held(Some(17)),
            RequestEffect::Held(None),
            RequestEffect::AlreadyClaimed,
        ] {
            let effect = prompt_request_effect(request);
            assert_eq!(effect.status, Status::Unconfirmed);
            assert_eq!(
                effect.reason.as_deref(),
                Some("blueprint_prompt_answer_held")
            );
        }
        assert_eq!(
            prompt_request_effect(RequestEffect::Held(Some(17)))
                .execution_id
                .as_deref(),
            Some("17")
        );
    }

    #[test]
    fn prompt_refusal_keeps_its_reason() {
        let effect = prompt_refused_effect("blueprint_prompt_identity_changed");
        assert_eq!(effect.status, Status::Failed);
        assert_eq!(
            effect.reason.as_deref(),
            Some("blueprint_prompt_identity_changed")
        );
    }
}
