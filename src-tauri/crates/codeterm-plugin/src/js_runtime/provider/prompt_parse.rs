use serde_json::Value;

use codeterm_types::pty::{PendingPrompt, PromptOption, PromptSection};

pub(super) fn parsed_prompt_from_js(obj: Value) -> Option<PendingPrompt> {
    let question = obj.get("question").and_then(|q| q.as_str())?.to_string();
    let options = obj.get("options").and_then(|o| o.as_array())?;
    let mut parsed = Vec::new();
    for opt in options {
        let pair = opt.as_array()?;
        if pair.len() != 2 {
            return None;
        }
        parsed.push(PromptOption {
            label: pair[0].as_str()?.to_string(),
            value: pair[1].as_str()?.to_string(),
        });
    }
    Some(PendingPrompt {
        request_id: None,
        question,
        options: parsed,
        sections: obj.get("sections").and_then(|s| s.as_array()).map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(PromptSection {
                        label: item.get("label")?.as_str()?.to_string(),
                        active: item
                            .get("active")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        completed: item
                            .get("completed")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    })
                })
                .collect()
        }),
        multi_select: obj
            .get("multiSelect")
            .or_else(|| obj.get("multi_select"))
            .and_then(|v| v.as_bool()),
        submit_available: obj
            .get("submitAvailable")
            .or_else(|| obj.get("submit_available"))
            .and_then(|v| v.as_bool()),
        selected_values: obj
            .get("selectedValues")
            .or_else(|| obj.get("selected_values"))
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect()
            }),
    })
}

impl super::JsProvider {
    pub(super) fn switch_confirmation_prompt_from_js(
        &self,
        screen: &str,
        selection: &str,
    ) -> Option<PendingPrompt> {
        if let Some(value) = self.try_call::<Option<Value>>(
            "parseSwitchConfirmationPrompt",
            &[serde_json::json!(screen), serde_json::json!(selection)],
        ) {
            return value.and_then(parsed_prompt_from_js);
        }
        self.native()
            .and_then(|provider| provider.parse_switch_confirmation_prompt(screen, selection))
    }

    pub(super) fn live_prompt_response_from_js(
        &self,
        option_index: usize,
        screen: &str,
    ) -> Option<Vec<Vec<u8>>> {
        let steps = self.try_call::<Vec<String>>(
            "buildPromptResponse",
            &[serde_json::json!(option_index), serde_json::json!(screen)],
        )?;
        use base64::Engine;
        let decoded: Option<Vec<_>> = steps
            .into_iter()
            .map(|step| {
                base64::engine::general_purpose::STANDARD
                    .decode(step.as_bytes())
                    .ok()
                    .filter(|bytes| !bytes.is_empty())
            })
            .collect();
        decoded.filter(|steps| !steps.is_empty())
    }

    pub(super) fn launch_onboarding_response_from_js(
        &self,
        screen: &str,
    ) -> Option<crate::provider::ProviderPromptKeyStep> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WireResponse {
            frame_key: String,
            step: String,
        }
        let response = self.try_call::<Option<WireResponse>>(
            "launchOnboardingResponse",
            &[serde_json::json!(screen)],
        )??;
        use base64::Engine;
        let step = base64::engine::general_purpose::STANDARD
            .decode(response.step.as_bytes())
            .ok()
            .filter(|bytes| !bytes.is_empty())?;
        (!response.frame_key.is_empty()).then_some(crate::provider::ProviderPromptKeyStep {
            frame_key: response.frame_key,
            step,
        })
    }

    pub(super) fn update_prompt_availability_from_js(
        &self,
        screen: &str,
    ) -> Option<crate::provider::ProviderUpdateAvailability> {
        if self.has_js("updatePromptAvailability") {
            self.try_call("updatePromptAvailability", &[serde_json::json!(screen)])
        } else {
            crate::provider::update_prompt::availability(screen)
        }
    }

    pub(super) fn update_prompt_safe_choice_from_js(
        &self,
        prompt: &PendingPrompt,
    ) -> Option<String> {
        let parsed = serde_json::json!({
            "question": prompt.question,
            "options": prompt.options.iter().map(|option| [&option.label, &option.value]).collect::<Vec<_>>(),
        });
        if let Some(value) = self.try_call::<Option<String>>("updatePromptSafeChoice", &[parsed]) {
            return value.filter(|value| {
                prompt
                    .options
                    .iter()
                    .filter(|option| &option.value == value)
                    .count()
                    == 1
            });
        }
        self.native()
            .and_then(|provider| provider.update_prompt_safe_choice(prompt))
    }
}
