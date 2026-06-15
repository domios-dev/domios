use crate::agent::registry::AgentRegistry;

fn valid_cli_binary(binary: &str) -> bool {
    !binary.is_empty()
        && !binary.chars().any(|c| {
            matches!(
                c,
                '/' | '\\' | ' ' | ';' | '|' | '&' | '$' | '`' | '<' | '>' | '\n'
            )
        })
}

pub(crate) fn cli_binary_path(binary: &str) -> Option<std::path::PathBuf> {
    if !valid_cli_binary(binary) {
        return None;
    }
    which::which(binary)
        .ok()
        .filter(|path| path.is_file())
        .or_else(|| {
            fallback_cli_binary_paths(binary)
                .into_iter()
                .find(|path| path.is_file())
        })
}

pub(crate) fn cli_binary_on_path(binary: &str) -> bool {
    cli_binary_path(binary).is_some()
}

fn fallback_cli_binary_paths(binary: &str) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    let home = dirs::home_dir();

    #[cfg(target_os = "macos")]
    {
        paths.push(std::path::PathBuf::from("/opt/homebrew/bin").join(binary));
        paths.push(std::path::PathBuf::from("/usr/local/bin").join(binary));
        if let Some(home) = home.as_ref() {
            paths.push(home.join(".local/bin").join(binary));
            paths.push(home.join(".npm-global/bin").join(binary));
            paths.push(home.join(".bun/bin").join(binary));
            paths.push(home.join(".cargo/bin").join(binary));
        }
    }
    #[cfg(target_os = "linux")]
    {
        paths.push(std::path::PathBuf::from("/usr/local/bin").join(binary));
        paths.push(std::path::PathBuf::from("/usr/bin").join(binary));
        paths.push(std::path::PathBuf::from("/snap/bin").join(binary));
        if let Some(home) = home.as_ref() {
            paths.push(home.join(".local/bin").join(binary));
            paths.push(home.join(".npm-global/bin").join(binary));
            paths.push(home.join(".bun/bin").join(binary));
            paths.push(home.join(".cargo/bin").join(binary));
        }
    }
    #[cfg(windows)]
    {
        let names = windows_cli_binary_names(binary);
        if let Ok(appdata) = std::env::var("APPDATA") {
            let dir = std::path::PathBuf::from(appdata).join("npm");
            paths.extend(names.iter().map(|name| dir.join(name)));
        }
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            let dir = std::path::PathBuf::from(localappdata).join("Programs");
            paths.extend(names.iter().map(|name| dir.join(name)));
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            let home = std::path::PathBuf::from(userprofile);
            let dirs = [
                home.join("scoop/shims"),
                home.join(".cargo/bin"),
                home.join(".bun/bin"),
                home.join(".local/bin"),
            ];
            for dir in dirs {
                paths.extend(names.iter().map(|name| dir.join(name)));
            }
        }
        let _ = home;
    }
    paths
}

#[cfg(windows)]
fn windows_cli_binary_names(binary: &str) -> Vec<String> {
    if std::path::Path::new(binary).extension().is_some() {
        return vec![binary.to_string()];
    }
    [".exe", ".cmd", ".bat", ".com", ""]
        .into_iter()
        .map(|ext| format!("{}{}", binary, ext))
        .collect()
}

pub(crate) fn provider_cli_install_warning(
    registry: &AgentRegistry,
    provider_id: &str,
) -> Result<Option<String>, String> {
    let Some((display_name, cli_binary, install_command)) =
        registry.with_provider(provider_id, |p| {
            (
                p.display_name().to_string(),
                p.cli_binary().map(str::to_string),
                p.install_command(crate::agent::provider::current_platform_tag()),
            )
        })
    else {
        let available = registry.provider_ids().join(", ");
        return Err(format!(
            "Unknown agent provider: '{}'. Available: [{}]",
            provider_id, available
        ));
    };
    let Some(binary) = cli_binary else {
        return Ok(None);
    };
    if !valid_cli_binary(&binary) {
        return Err(format!(
            "Provider '{}' declares invalid CLI binary '{}'",
            provider_id, binary
        ));
    }
    if cli_binary_path(&binary).is_some() {
        return Ok(None);
    }
    let install_hint = install_command
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "No install command is available for this platform.".to_string());
    Ok(Some(format!(
        "Provider not installed: {} requires CLI `{}` on PATH. Install command: {}",
        display_name, binary, install_hint
    )))
}
