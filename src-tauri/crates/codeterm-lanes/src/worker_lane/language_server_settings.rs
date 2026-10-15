use std::path::{Component, Path};

use serde_json::{json, Value};

use codeterm_plugin::provider::WorkerLaneSetting;

const SETTINGS_FILES: [&str; 2] = [".vscode/settings.json", "rust-analyzer.toml"];

pub(super) fn configure(cwd: &str, provider_settings: &[WorkerLaneSetting]) -> Result<(), String> {
    let vscode = json!({
        "rust-analyzer.checkOnSave": false,
        "rust-analyzer.cargo.buildScripts.enable": false,
        "rust-analyzer.cargo.buildScripts.rebuildOnSave": false,
        "rust-analyzer.procMacro.enable": false
    });
    // Project TOML cannot disable the global procMacro option, which also requests builds.
    let analyzer = json!({
        "checkOnSave": false,
        "cargo": {
            "buildScripts": {
                "enable": false,
                "rebuildOnSave": false,
                "overrideCommand": ["git", "--version"]
            }
        }
    });
    let mut written: Vec<&str> = Vec::new();
    for (relative, patch) in [(SETTINGS_FILES[0], &vscode), (SETTINGS_FILES[1], &analyzer)]
        .into_iter()
        .chain(
            provider_settings
                .iter()
                .map(|setting| (setting.path.as_str(), &setting.patch)),
        )
    {
        refuse_escaping_path(relative)?;
        write_settings(cwd, relative, patch)?;
        hide_tracked_settings(cwd, relative)?;
        if !written.contains(&relative) {
            written.push(relative);
        }
    }
    let ignore = Path::new(cwd).join(".gitignore");
    refuse_symlink_path(cwd, ".gitignore")?;
    let mut content = read_optional(&ignore)?;
    content.push_str(
        "\n# Worker lane language-server settings stay out of integration.\n/.gitignore\n",
    );
    for relative in written {
        content.push('/');
        content.push_str(relative);
        content.push('\n');
    }
    std::fs::write(&ignore, content).map_err(|error| format!("{}: {error}", ignore.display()))?;
    hide_tracked_settings(cwd, ".gitignore")
}

fn refuse_escaping_path(relative: &str) -> Result<(), String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "worker lane settings path must stay inside the lane: {relative}"
        ));
    }
    Ok(())
}

fn hide_tracked_settings(cwd: &str, relative: &str) -> Result<(), String> {
    if !super::git_value(cwd, ["ls-files", "--", relative])?.is_empty() {
        super::run_git(cwd, ["update-index", "--skip-worktree", "--", relative])?;
    }
    Ok(())
}

fn refuse_symlink_path(cwd: &str, relative: &str) -> Result<(), String> {
    let mut path = Path::new(cwd).to_path_buf();
    for component in Path::new(relative).components() {
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "worker lane settings refuse symlink: {}",
                    path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
    Ok(())
}

fn read_optional(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn write_settings(cwd: &str, relative: &str, patch: &Value) -> Result<(), String> {
    refuse_symlink_path(cwd, relative)?;
    let path = Path::new(cwd).join(relative);
    let content = read_optional(&path)?;
    let is_toml = relative.ends_with(".toml");
    let mut settings = if content.trim().is_empty() {
        json!({})
    } else if is_toml {
        let parsed: toml::Value =
            toml::from_str(&content).map_err(|error| format!("{}: {error}", path.display()))?;
        serde_json::to_value(parsed).map_err(|error| error.to_string())?
    } else {
        serde_json::from_str(&content).map_err(|error| format!("{}: {error}", path.display()))?
    };
    merge_settings(&mut settings, patch)?;
    let content = if is_toml {
        toml::to_string_pretty(&settings).map_err(|error| error.to_string())?
    } else {
        serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, content).map_err(|error| format!("{}: {error}", path.display()))
}

fn merge_settings(settings: &mut Value, patch: &Value) -> Result<(), String> {
    let target = settings
        .as_object_mut()
        .ok_or_else(|| "worker lane settings must be an object".to_string())?;
    let patch = patch
        .as_object()
        .ok_or_else(|| "worker lane settings patch must be an object".to_string())?;
    for (key, value) in patch {
        if value.is_object() {
            let target = target.entry(key.clone()).or_insert_with(|| json!({}));
            merge_settings(target, value)?;
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
