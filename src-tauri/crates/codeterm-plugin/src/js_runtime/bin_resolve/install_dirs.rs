use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

const MAX_NPMRC_BYTES: u64 = 64 * 1024;

pub fn expand_install_dirs(
    templates: &[String],
    env: &dyn Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for template in templates {
        if let Some(dir) = expand_install_dir(template, env, home) {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs
}

pub fn expand_install_dir(
    template: &str,
    env: &dyn Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let trimmed = template.trim();
    if trimmed.is_empty() || trimmed.contains(['\0', '\r', '\n']) {
        return None;
    }
    if trimmed == "~" {
        return home.map(Path::to_path_buf);
    }
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        return join_safe_subpath(home?.to_path_buf(), rest);
    }
    if let Some(after_pct) = trimmed.strip_prefix('%') {
        let end = after_pct.find('%')?;
        let var = &after_pct[..end];
        if !valid_env_name(var) {
            return None;
        }
        let rest = &after_pct[end + 1..];
        if !rest.is_empty() && !rest.starts_with(['/', '\\']) {
            return None;
        }
        let base = PathBuf::from(env(var).filter(|value| !value.is_empty())?);
        return join_safe_subpath(base, rest.trim_start_matches(['/', '\\']));
    }
    if let Some(after_brace) = trimmed.strip_prefix("${") {
        let end = after_brace.find('}')?;
        let var = &after_brace[..end];
        if !valid_env_name(var) {
            return None;
        }
        let rest = &after_brace[end + 1..];
        if !rest.is_empty() && !rest.starts_with(['/', '\\']) {
            return None;
        }
        let base = PathBuf::from(env(var).filter(|value| !value.is_empty())?);
        return join_safe_subpath(base, rest.trim_start_matches(['/', '\\']));
    }
    if let Some(after_dollar) = trimmed.strip_prefix('$') {
        let end = after_dollar
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after_dollar.len());
        let var = &after_dollar[..end];
        if !valid_env_name(var) {
            return None;
        }
        let rest = &after_dollar[end..];
        if !rest.is_empty() && !rest.starts_with(['/', '\\']) {
            return None;
        }
        let base = PathBuf::from(env(var).filter(|value| !value.is_empty())?);
        return join_safe_subpath(base, rest.trim_start_matches(['/', '\\']));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    Some(path)
}

pub(super) fn npm_global_bin_dirs(
    env: &dyn Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
    windows: bool,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for key in ["NPM_CONFIG_PREFIX", "npm_config_prefix"] {
        if let Some(prefix) = env(key).filter(|value| !value.is_empty()) {
            push_npm_prefix(&mut dirs, PathBuf::from(prefix), windows);
        }
    }
    if let Some(home) = home {
        if let Some(prefix) = read_npmrc_prefix(&home.join(".npmrc"), Some(home)) {
            push_npm_prefix(&mut dirs, prefix, windows);
        }
    }
    if windows {
        if let Some(appdata) = env("APPDATA").filter(|value| !value.is_empty()) {
            push_unique(&mut dirs, PathBuf::from(appdata).join("npm"));
        }
    }
    dirs
}

fn push_npm_prefix(dirs: &mut Vec<PathBuf>, prefix: PathBuf, windows: bool) {
    let bin_dir = if windows { prefix } else { prefix.join("bin") };
    push_unique(dirs, bin_dir);
}

fn push_unique(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
        dirs.push(dir);
    }
}

fn read_npmrc_prefix(path: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_NPMRC_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    parse_npmrc_prefix(&content, home)
}

pub(super) fn parse_npmrc_prefix(content: &str, home: Option<&Path>) -> Option<PathBuf> {
    let mut last = None;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("prefix") {
            continue;
        }
        let value = value.trim().trim_matches(['"', '\'']).trim();
        if value.is_empty() || value.contains(['\0', '\r', '\n']) {
            continue;
        }
        let resolved = if let Some(rest) = value
            .strip_prefix("~/")
            .or_else(|| value.strip_prefix("~\\"))
        {
            join_safe_subpath(home?.to_path_buf(), rest)?
        } else {
            PathBuf::from(value)
        };
        last = Some(resolved);
    }
    last
}

fn valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn join_safe_subpath(mut base: PathBuf, subpath: &str) -> Option<PathBuf> {
    for part in subpath.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        base.push(part);
    }
    Some(base)
}

#[cfg(test)]
mod tests;
