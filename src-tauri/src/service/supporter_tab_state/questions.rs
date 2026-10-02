use std::collections::BTreeMap;

use codeterm_types::decision::{DecisionQuestion, NamedDecisionQuestion, NoulCriteria};

use super::{criteria, TabStateMetadata, INSTRUCTIONS};

pub(crate) fn observation_questions(
    _metadata: &TabStateMetadata,
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
    questions
}
