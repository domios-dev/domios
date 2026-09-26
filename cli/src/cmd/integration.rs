use crate::args::integration::IntegrationAction;
use crate::cmd::print::print_object;

/// The one REST request each verb makes, shared by the local path and `--mesh`.
pub fn api_request(action: &IntegrationAction) -> (String, String, Option<String>) {
    match action {
        IntegrationAction::Install {
            provider,
            precompact_memory_export,
            no_precompact_memory_export,
        } => (
            "POST".into(),
            {
                let mut path = format!(
                    "/api/setup/hooks/install?provider={}",
                    urlencoding::encode(provider)
                );
                let requested_precompact = if *precompact_memory_export {
                    Some(true)
                } else if *no_precompact_memory_export {
                    Some(false)
                } else {
                    None
                };
                if let Some(value) = requested_precompact {
                    path.push_str(&format!("&precompact_memory_export={value}"));
                }
                path
            },
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
                "/api/setup/hooks/install?provider=codex".to_string(),
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
    fn install_omits_precompact_when_not_specified() {
        let (_, path, _) = api_request(&action(&[
            "codeterm",
            "integration",
            "install",
            "--provider",
            "claude",
        ]));
        assert_eq!(path, "/api/setup/hooks/install?provider=claude");
    }

    #[test]
    fn install_can_explicitly_disable_precompact_export() {
        let (_, path, _) = api_request(&action(&[
            "codeterm",
            "integration",
            "install",
            "--provider",
            "claude",
            "--no-precompact-memory-export",
        ]));
        assert!(path.ends_with("provider=claude&precompact_memory_export=false"));
    }

    #[test]
    fn install_rejects_conflicting_precompact_flags() {
        assert!(crate::Cli::try_parse_from([
            "codeterm",
            "integration",
            "install",
            "--provider",
            "claude",
            "--precompact-memory-export",
            "--no-precompact-memory-export",
        ])
        .is_err());
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
}
