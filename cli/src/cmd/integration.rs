use crate::args::integration::IntegrationAction;
use crate::cmd::print::print_object;

/// The one REST request each verb makes, shared by the local path and `--mesh`.
pub fn api_request(action: &IntegrationAction) -> (String, String, Option<String>) {
    match action {
        IntegrationAction::Install {
            provider,
            precompact_memory_export,
        } => (
            "POST".into(),
            format!(
                "/api/setup/hooks/install?provider={}&precompact_memory_export={}",
                urlencoding::encode(provider),
                precompact_memory_export
            ),
            None,
        ),
        IntegrationAction::Status { provider } => (
            "GET".into(),
            format!(
                "/api/setup/hooks?provider={}",
                urlencoding::encode(provider)
            ),
            None,
        ),
        IntegrationAction::SweepProfiles {
            provider,
            confirm,
            include_legacy,
        } => (
            "POST".into(),
            format!(
                "/api/integration/sweep-profiles?provider={}&confirm={}&include_legacy={}",
                urlencoding::encode(provider),
                confirm,
                include_legacy
            ),
            None,
        ),
    }
}

pub fn handle_integration(json: bool, action: &IntegrationAction) {
    let (base, _) = crate::read_api_config();
    let http = crate::build_local_api().unwrap_or_else(|error| {
        eprintln!("Error: failed to connect to local API: {error}");
        std::process::exit(1);
    });
    let (method, path, _) = api_request(action);
    let url = format!("{base}{path}");
    let request = if method == "POST" {
        http.post(url)
    } else {
        http.get(url)
    };
    let result = request
        .send()
        .map_err(|error| error.to_string())
        .and_then(|response| {
            let status = response.status();
            let body = response.text().map_err(|error| error.to_string())?;
            if !status.is_success() {
                return Err(format!("{status}: {body}"));
            }
            serde_json::from_str::<serde_json::Value>(&body).map_err(|error| error.to_string())
        });
    match result {
        Ok(value) if json => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Ok(value) => print_object(&value),
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn action(argv: &[&str]) -> IntegrationAction {
        match crate::Cli::try_parse_from(argv).expect("parses").command {
            crate::Commands::Integration { action } => action,
            _ => panic!("not an integration command"),
        }
    }

    #[test]
    fn install_posts_the_consented_install_for_the_named_provider() {
        let request = api_request(&action(&[
            "codeterm",
            "integration",
            "install",
            "--provider",
            "codex",
        ]));
        assert_eq!(
            request,
            (
                "POST".to_string(),
                "/api/setup/hooks/install?provider=codex&precompact_memory_export=false"
                    .to_string(),
                None
            )
        );
    }

    #[test]
    fn install_carries_the_precompact_export_choice() {
        let (_, path, _) = api_request(&action(&[
            "codeterm",
            "integration",
            "install",
            "--provider",
            "claude",
            "--precompact-memory-export",
        ]));
        assert!(path.ends_with("provider=claude&precompact_memory_export=true"));
    }

    #[test]
    fn status_reads_the_provider_integration_state() {
        let request = api_request(&action(&[
            "codeterm",
            "integration",
            "status",
            "--provider",
            "a b",
        ]));
        assert_eq!(
            request,
            (
                "GET".to_string(),
                "/api/setup/hooks?provider=a%20b".to_string(),
                None
            )
        );
    }

    #[test]
    fn install_requires_a_provider() {
        assert!(crate::Cli::try_parse_from(["codeterm", "integration", "install"]).is_err());
    }

    #[test]
    fn sweep_profiles_is_dry_run_by_default() {
        let request = api_request(&action(&[
            "codeterm",
            "integration",
            "sweep-profiles",
            "--provider",
            "codex",
        ]));
        assert_eq!(
            request,
            (
                "POST".to_string(),
                "/api/integration/sweep-profiles?provider=codex&confirm=false&include_legacy=false"
                    .to_string(),
                None
            )
        );
    }

    #[test]
    fn sweep_profiles_requires_both_explicit_deletion_flags() {
        let request = api_request(&action(&[
            "codeterm",
            "integration",
            "sweep-profiles",
            "--provider",
            "codex",
            "--confirm",
            "--include-legacy",
        ]));
        assert!(request.1.ends_with("confirm=true&include_legacy=true"));
    }
}
