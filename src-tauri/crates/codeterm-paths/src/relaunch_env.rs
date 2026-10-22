use std::ffi::OsString;
use std::path::Path;

fn integration_path(value: &str, integration: Option<&str>) -> bool {
    let normalized = value.replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');
    integration.is_some_and(|dir| {
        let dir = dir.replace('\\', "/");
        let dir = dir.trim_end_matches('/');
        normalized == dir
            || normalized
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/'))
    }) || normalized
        .split('/')
        .collect::<Vec<_>>()
        .windows(2)
        .any(|parts| {
            (parts[0] == ".domios"
                || parts[0].starts_with(".domios-dev")
                || parts[0] == ".codeterm"
                || parts[0].starts_with(".codeterm-dev"))
                && parts[1] == "shell-integration"
        })
}

pub fn filter_relaunch_env<I, K, V>(env: I) -> Vec<(K, V)>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let env: Vec<_> = env.into_iter().collect();
    let integration = env
        .iter()
        .find(|(key, _)| {
            key.as_ref()
                .eq_ignore_ascii_case("CODETERM_INTEGRATION_ZDOTDIR")
        })
        .map(|(_, value)| value.as_ref());
    let integration = integration.map(str::to_owned);
    env.into_iter()
        .filter(|(key, value)| {
            let key = key.as_ref().to_ascii_uppercase();
            !(key == "CODETERM"
                || key.starts_with("CODETERM_")
                || match key.as_str() {
                    "ZDOTDIR" | "BASH_ENV" | "ENV" => {
                        integration_path(value.as_ref(), integration.as_deref())
                    }
                    "TERM_PROGRAM" => value.as_ref().eq_ignore_ascii_case("CodeTerm"),
                    "TERM"
                    | "CLICOLOR"
                    | "COLORTERM"
                    | "TERM_PROGRAM_VERSION"
                    | "TERM_SESSION_ID"
                    | "WINDOWID"
                    | "WT_SESSION"
                    | "WT_PROFILE_ID"
                    | "SHLVL"
                    | "PROMPT_COMMAND" => true,
                    _ => false,
                })
        })
        .collect()
}

pub fn clean_relaunch_env_from(env: Vec<(OsString, OsString)>) -> Vec<(OsString, OsString)> {
    let text: Vec<_> = env
        .iter()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let kept = filter_relaunch_env(text);
    let integration = env
        .iter()
        .find(|(key, _)| key == "CODETERM_INTEGRATION_ZDOTDIR")
        .map(|(_, value)| value.to_string_lossy().into_owned());
    let original = env
        .iter()
        .find(|(key, _)| key == "CODETERM_ORIG_ZDOTDIR")
        .map(|(_, value)| value.clone())
        .filter(|value| {
            !value.is_empty() && !integration_path(&value.to_string_lossy(), integration.as_deref())
        });
    let mut clean = Vec::new();
    for (key, value) in env {
        if kept.iter().any(|(name, _)| key == OsString::from(name)) {
            if key == "XDG_DATA_DIRS" {
                if let Ok(paths) =
                    std::env::join_paths(std::env::split_paths(&value).filter(|path| {
                        !integration_path(&path.to_string_lossy(), integration.as_deref())
                    }))
                {
                    if !paths.is_empty() {
                        clean.push((key, paths));
                    }
                }
            } else {
                clean.push((key, value));
            }
        }
    }
    if !clean.iter().any(|(key, _)| key == "ZDOTDIR") {
        if let Some(original) = original {
            clean.push(("ZDOTDIR".into(), original));
        }
    }
    clean
}

pub fn clean_relaunch_env() -> Vec<(OsString, OsString)> {
    clean_relaunch_env_from(std::env::vars_os().collect())
}

pub fn apply_relaunch_env(command: &mut std::process::Command) {
    command.env_clear().envs(clean_relaunch_env());
}

#[cfg(windows)]
pub fn scrub_before_installer_exit() {
    let clean = clean_relaunch_env();
    for (key, _) in std::env::vars_os() {
        if !clean.iter().any(|(name, _)| name == &key) {
            std::env::remove_var(key);
        }
    }
    for (key, value) in clean {
        std::env::set_var(key, value);
    }
}

pub fn user_zdotdir(value: &Path, integration: &Path, home: &Path) -> std::path::PathBuf {
    let same = value == integration
        || std::fs::canonicalize(value)
            .ok()
            .zip(std::fs::canonicalize(integration).ok())
            .is_some_and(|(a, b)| a == b);
    if value.as_os_str().is_empty() || same {
        home.to_owned()
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests;
