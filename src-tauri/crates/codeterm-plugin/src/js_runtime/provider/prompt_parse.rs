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
