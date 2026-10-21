use std::collections::BTreeMap;

use codeterm_decision::{DecisionQuestion, NamedDecisionQuestion, NoulCriteria};

use super::{criteria, TabStateMetadata, INSTRUCTIONS};

pub(crate) fn observation_questions(
    metadata: &TabStateMetadata,
    safe_choice: Option<DecisionQuestion>,
) -> BTreeMap<String, NamedDecisionQuestion> {
    let mut questions = BTreeMap::from([
        ("label".into(), NamedDecisionQuestion {
            instructions: INSTRUCTIONS.to_owned(),
            question: DecisionQuestion::Choice { options: criteria().clone() },
        }),
        ("prompt_visible".into(), NamedDecisionQuestion {
            instructions: "Is an interactive prompt awaiting a response on the current screen?".into(),
            question: DecisionQuestion::Noul { criteria: Some(NoulCriteria {
                true_def: "A currently visible interactive prompt is waiting for input or a choice.".into(),
                false_def: "There is no currently visible interactive prompt waiting for input; historical transcript prompts do not count.".into(),
            }) },
        }),
    ]);
    if let Some(question) = safe_choice {
        questions.insert("safe_choice".into(), NamedDecisionQuestion {
            instructions: "Choose the provider-declared safe option for this currently visible prompt, or abstain if no option is safe.".into(),
            question,
        });
    }
    let event = metadata.context["observation_event"]
        .as_str()
        .unwrap_or_default();
    if matches!(
        event,
        "prompt_candidate" | "startup_escalation" | "anomaly" | "user_delivery_readiness"
    ) {
        add_choice(
            &mut questions,
            "input_area",
            "What best explains the apparent input area?",
            &[
                ("composer", "The current input area is a composer."),
                (
                    "active_modal",
                    "The current input area belongs to an active modal.",
                ),
                (
                    "historical_or_draft_text",
                    "The apparent prompt is historical or draft text.",
                ),
                ("other", "The current input area cannot be determined."),
            ],
        );
    }
    if matches!(event, "busy_without_output" | "anomaly") {
        add_choice(
            &mut questions,
            "progress",
            "What best explains the apparent lack of progress?",
            &[
                (
                    "normal_work",
                    "Current evidence is consistent with normal work.",
                ),
                ("waiting_for_input", "The current tab is waiting for input."),
                (
                    "apparently_idle",
                    "The current tab appears idle without proving task success.",
                ),
                (
                    "repeated_error",
                    "Current attributable evidence shows repeated errors.",
                ),
                ("other", "Progress cannot be determined from this event."),
            ],
        );
    }
    if matches!(event, "startup_escalation" | "anomaly") {
        add_choice(
            &mut questions,
            "provider_failure",
            "Does this event describe a current provider failure?",
            &[
                (
                    "auth_required",
                    "A current provider event requires authentication.",
                ),
                (
                    "model_unavailable",
                    "A current provider event reports an unavailable model.",
                ),
                (
                    "quota_exhausted",
                    "A current provider event reports exhausted quota.",
                ),
                (
                    "configuration_error",
                    "A current provider event reports a configuration error.",
                ),
                (
                    "historical_text",
                    "The apparent failure is historical text.",
                ),
                (
                    "other",
                    "No current attributable provider failure can be determined.",
                ),
            ],
        );
    }
    questions
}

fn add_choice(
    questions: &mut BTreeMap<String, NamedDecisionQuestion>,
    name: &str,
    instructions: &str,
    options: &[(&str, &str)],
) {
    questions.insert(
        name.into(),
        NamedDecisionQuestion {
            instructions: instructions.into(),
            question: DecisionQuestion::Choice {
                options: options
                    .iter()
                    .map(|(key, criteria)| (key.to_string(), criteria.to_string()))
                    .collect(),
            },
        },
    );
}
