use serde_json::Value;

pub(crate) fn resolve_remote_spawn(
    descriptors: &Value,
    requested: &str,
    environment: &str,
) -> Result<(String, String), String> {
    let providers = descriptors.as_array().ok_or("Invalid provider catalogue")?;
    let candidates: Vec<_> = providers
        .iter()
        .filter(|provider| {
            provider["transport"] == "chat_backend"
                && provider["remote_task"].is_object()
                && (provider["id"] == requested
                    || provider["remote_task"]["localProviderId"] == requested)
        })
        .collect();
    let [provider] = candidates.as_slice() else {
        return Err("Choose a provider with one declared remote task backend".into());
    };
    let remote = &provider["remote_task"];
    if remote["availability"]["status"] != "available" {
        return Err(remote["availability"]["reason"]
            .as_str()
            .unwrap_or("Remote execution is unavailable")
            .into());
    }
    let environments = remote["environments"]
        .as_array()
        .ok_or("No remote environments are available; connect the owning account")?;
    let exact = environments.iter().find(|entry| entry["id"] == environment);
    let matches: Vec<_> = environments
        .iter()
        .filter(|entry| {
            entry["label"]
                .as_str()
                .is_some_and(|label| label.eq_ignore_ascii_case(environment))
        })
        .collect();
    let selected = match (exact, matches.as_slice()) {
        (Some(entry), _) => entry,
        (None, [entry]) => *entry,
        _ => return Err("Choose an available, unambiguous remote environment ID".into()),
    };
    let provider_id = provider["id"]
        .as_str()
        .ok_or("Missing remote provider ID")?;
    let environment_id = selected["id"].as_str().ok_or("Missing environment ID")?;
    Ok((provider_id.into(), environment_id.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde_json::json;

    #[test]
    fn remote_location_requires_an_environment_and_preserves_local_defaults() {
        assert!(crate::Cli::try_parse_from([
            "codeterm",
            "agent",
            "spawn",
            "fixture",
            "--location",
            "remote"
        ])
        .is_err());
        assert!(crate::Cli::try_parse_from([
            "codeterm",
            "agent",
            "spawn",
            "fixture",
            "--environment",
            "env"
        ])
        .is_err());
        let parsed = crate::Cli::try_parse_from([
            "codeterm",
            "agent",
            "spawn",
            "fixture",
            "--location",
            "remote",
            "--environment",
            "env",
        ])
        .unwrap();
        let crate::Commands::Agent {
            action:
                crate::AgentAction::Spawn {
                    location,
                    environment,
                    ..
                },
        } = parsed.command
        else {
            panic!("expected spawn")
        };
        assert_eq!(location, Some(crate::args::agent::SpawnLocation::Remote));
        assert_eq!(environment.as_deref(), Some("env"));
    }

    #[test]
    fn remote_spawn_uses_capability_relationship_and_environment_identity() {
        let catalogue = json!([{"id":"renamed-remote", "transport":"chat_backend", "remote_task":{
            "localProviderId":"renamed-local", "availability":{"status":"available"},
            "environments":[{"id":"env-1","label":"Fixture environment"}]
        }}]);
        assert_eq!(
            resolve_remote_spawn(&catalogue, "renamed-local", "fixture ENVIRONMENT").unwrap(),
            ("renamed-remote".into(), "env-1".into())
        );
        assert!(resolve_remote_spawn(&catalogue, "foreign", "env-1").is_err());
        assert!(resolve_remote_spawn(&catalogue, "renamed-local", "foreign").is_err());
        let mut unavailable = catalogue.clone();
        unavailable[0]["remote_task"]["availability"] =
            json!({"status":"unavailable","reason":"fixture account unavailable"});
        assert_eq!(
            resolve_remote_spawn(&unavailable, "renamed-local", "env-1").unwrap_err(),
            "fixture account unavailable"
        );
        let mut empty = catalogue;
        empty[0]["remote_task"]["environments"] = json!([]);
        assert!(resolve_remote_spawn(&empty, "renamed-local", "env-1").is_err());
    }
}
