use crate::provider::{ModelSwitchCommand, ModelSwitchCommands, ModelSwitchTranscript};

pub(super) fn commands() -> ModelSwitchCommands {
    ModelSwitchCommands {
        transcript: ModelSwitchTranscript {
            record_type: "user".into(),
            content_path: vec!["message".into(), "content".into()],
            prefix: "<local-command-stdout>".into(),
            suffix: "</local-command-stdout>".into(),
        },
        model: ModelSwitchCommand {
            command: "/model {model}".into(),
            confirmation: "(?i)^Set model to .*?(?:{model}|{display_name})(?:[^a-z0-9_-]|$)".into(),
        },
        effort: Some(ModelSwitchCommand {
            command: "/effort {effort}".into(),
            confirmation: "(?i)^Set effort level to {effort}(?:[^a-z0-9_-]|$)".into(),
        }),
    }
}
