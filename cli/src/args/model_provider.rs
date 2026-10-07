use clap::{Subcommand, ValueEnum};

#[derive(Clone, Debug, ValueEnum)]
pub enum EndpointKind {
    OpenaiCompatible,
    Anthropic,
}

#[derive(Debug, Subcommand)]
pub enum ModelProviderAction {
    /// Manage Domios-owned provider presets
    Preset {
        #[command(subcommand)]
        action: ModelPresetAction,
    },
    /// Add an endpoint; pipe the key on stdin. Omit --model to discover models.
    Add {
        #[arg(long)]
        name: String,
        #[arg(long, value_enum)]
        kind: EndpointKind,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        secret: String,
        #[arg(long = "model")]
        models: Vec<String>,
    },
    /// Remove a Domios-owned endpoint and its owned secret
    Remove {
        #[arg(long)]
        name: String,
    },
    /// List Domios-owned endpoints without key values
    List,
    /// Check an endpoint within a bounded deadline and return its models
    Test {
        #[arg(long)]
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ModelPresetAction {
    /// Create or replace an owned model, prompt and parameter preset
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        temperature: Option<f64>,
        #[arg(long = "top-p")]
        top_p: Option<f64>,
        #[arg(long)]
        steps: Option<u32>,
    },
    /// Remove only a Domios-owned preset
    Remove {
        #[arg(long)]
        name: String,
    },
    /// List Domios-owned presets
    List,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::cli::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn model_provider_preset_parse_params_and_spawn_selection() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "model-provider",
            "preset",
            "add",
            "--name",
            "demo",
            "--model",
            "endpoint/model",
            "--prompt",
            "opaque",
            "--temperature",
            "0.2",
            "--top-p",
            "0.8",
            "--steps",
            "12",
        ])
        .unwrap();
        let Commands::ModelProvider {
            action:
                ModelProviderAction::Preset {
                    action:
                        ModelPresetAction::Add {
                            name,
                            model,
                            prompt,
                            temperature,
                            top_p,
                            steps,
                        },
                },
        } = parsed.command
        else {
            panic!("wrong command");
        };
        assert_eq!(
            (name.as_str(), model.as_str(), prompt.as_str()),
            ("demo", "endpoint/model", "opaque")
        );
        assert_eq!(
            (temperature, top_p, steps),
            (Some(0.2), Some(0.8), Some(12))
        );
        assert!(Cli::try_parse_from(["codeterm", "model-provider", "preset", "list"]).is_ok());
        assert!(Cli::try_parse_from([
            "codeterm",
            "model-provider",
            "preset",
            "remove",
            "--name",
            "demo"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "codeterm", "agent", "spawn", "opencode", "--preset", "demo"
        ])
        .is_ok());
    }

    #[test]
    fn model_provider_parse_repeatable_models_and_no_key_argument() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "model-provider",
            "add",
            "--name",
            "demo",
            "--kind",
            "openai-compatible",
            "--base-url",
            "http://localhost:1234/v1",
            "--secret",
            "slot",
            "--model",
            "a",
            "--model",
            "b",
        ])
        .unwrap();
        let Commands::ModelProvider {
            action: ModelProviderAction::Add { models, .. },
        } = parsed.command
        else {
            panic!("wrong command");
        };
        assert_eq!(models, ["a", "b"]);
        assert!(
            Cli::try_parse_from(["codeterm", "model-provider", "list", "--key", "forbidden"])
                .is_err()
        );
        assert!(Cli::try_parse_from([
            "codeterm",
            "model-provider",
            "test",
            "--name",
            "demo",
            "--json"
        ])
        .is_ok());
    }
}
