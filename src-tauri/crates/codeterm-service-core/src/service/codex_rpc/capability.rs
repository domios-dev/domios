use codeterm_plugin::provider::StructuredControl;

pub fn enabled(
    capability: Option<&StructuredControl>,
    config: &crate::config::schema::ProviderConfig,
) -> bool {
    capability.is_some_and(|cap| cap.protocol == "codex-app-server")
        && config
            .extra
            .get("structured_control_enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true)
}

pub fn version_supported(user_agent: &str, minimum: &str) -> bool {
    let version = user_agent
        .split_once('/')
        .map(|(_, rest)| rest)
        .unwrap_or(user_agent)
        .split_whitespace()
        .next()
        .unwrap_or("");
    match (
        semver::Version::parse(version),
        semver::Version::parse(minimum),
    ) {
        (Ok(version), Ok(minimum)) => version >= minimum,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_rpc_capability_defaults_on_and_checks_protocol() {
        let cap = StructuredControl {
            protocol: "codex-app-server".into(),
            min_version: "0.160.0".into(),
        };
        let mut config = crate::config::schema::ProviderConfig::default();
        assert!(enabled(Some(&cap), &config));
        config.extra.insert(
            "structured_control_enabled".into(),
            toml::Value::Boolean(false),
        );
        assert!(!enabled(Some(&cap), &config));
        config.extra.insert(
            "structured_control_enabled".into(),
            toml::Value::Boolean(true),
        );
        assert!(enabled(Some(&cap), &config));
        assert!(!enabled(None, &config));
        assert!(!enabled(
            Some(&StructuredControl {
                protocol: "other".into(),
                ..cap
            }),
            &config
        ));
    }
    #[test]
    fn codex_rpc_version_gate() {
        assert!(version_supported("codex/0.160.0 (Windows)", "0.160.0"));
        assert!(!version_supported("codex/0.159.0", "0.160.0"));
        assert!(!version_supported("unknown", "0.160.0"));
    }
}
