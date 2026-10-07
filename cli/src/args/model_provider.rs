use clap::{Subcommand, ValueEnum};

#[derive(Clone, Debug, ValueEnum)]
pub enum EndpointKind {
    OpenaiCompatible,
    Anthropic,
}

#[derive(Debug, Subcommand)]
pub enum ModelProviderAction {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::cli::{Cli, Commands};
    use clap::Parser;

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
