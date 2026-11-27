use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use parking_lot::Mutex;
use std::time::SystemTime;

const MAX_NPMRC_BYTES: u64 = 64 * 1024;
const MAX_WILDCARD_SCAN: usize = 256;
const MAX_WILDCARD_DIRS: usize = 64;

pub fn expand_install_dirs(
    templates: &[String],
    env: &dyn Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    expand_install_dirs_with_reader(templates, env, home, &read_subdirs)
}

pub fn expand_install_dirs_with_reader(
    templates: &[String],
    env: &dyn Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
    read_subdirs: &dyn Fn(&Path) -> Vec<String>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for template in templates {
        for dir in expand_install_dir_entries(template, env, home, read_subdirs) {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs
}

fn expand_install_dir_entries(
    template: &str,
    env: &dyn Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
    read_subdirs: &dyn Fn(&Path) -> Vec<String>,
) -> Vec<PathBuf> {
    let trimmed = template.trim();
    if trimmed.is_empty() || trimmed.contains(['\0', '\r', '\n']) {
        return Vec::new();
    }
    if !trimmed.contains('*') {
        return expand_install_dir(trimmed, env, home).into_iter().collect();
    }
    let Some((prefix, suffix)) = split_wildcard_template(trimmed) else {
        return Vec::new();
    };
    let Some(base) = expand_install_dir(prefix, env, home) else {
        return Vec::new();
    };
    let mut children = read_subdirs(&base);
    children.sort_by(|left, right| {
        let left_ver = semver::Version::parse(left.trim_start_matches('v')).ok();
        let right_ver = semver::Version::parse(right.trim_start_matches('v')).ok();
        right_ver.cmp(&left_ver).then_with(|| right.cmp(left))
    });
    children.truncate(MAX_WILDCARD_DIRS);
    let mut expanded = Vec::new();
    for child in children {
        if child.is_empty()
            || matches!(child.as_str(), "." | "..")
            || child.contains(['/', '\\', ':', '\0', '\r', '\n'])
        {
            continue;
        }
        if let Some(path) = join_safe_subpath(base.join(child), suffix) {
            if !expanded.contains(&path) {
                expanded.push(path);
            }
        }
    }
    expanded
}

fn split_wildcard_template(trimmed: &str) -> Option<(&str, &str)> {
    if trimmed.matches('*').count() != 1 {
        return None;
    }
    let index = trimmed.find('*')?;
    let bytes = trimmed.as_bytes();
    if index == 0 || !matches!(bytes[index - 1], b'/' | b'\\') {
        return None;
    }
    if index + 1 < bytes.len() && !matches!(bytes[index + 1], b'/' | b'\\') {
        return None;
    }
    let prefix = &trimmed[..index - 1];
    if prefix.is_empty() {
        return None;
    }
    let suffix = trimmed[index + 1..].trim_start_matches(['/', '\\']);
    Some((prefix, suffix))
}

fn read_subdirs(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .take(MAX_WILDCARD_SCAN)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let is_dir = file_type.is_dir()
                || (file_type.is_symlink() && std::fs::metadata(entry.path()).ok()?.is_dir());
            is_dir
                .then(|| entry.file_name().to_str().map(str::to_owned))
                .flatten()
        })
        .collect()
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
        return home
            .map(Path::to_path_buf)
            .filter(|path| valid_install_path(path));
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
    valid_install_path(&path).then_some(path)
}

fn valid_install_path(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| component == Component::ParentDir)
        && !path
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(u8::is_ascii_control)
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
    if valid_install_path(&dir) && !dirs.contains(&dir) {
        dirs.push(dir);
    }
}

struct CachedPrefix {
    path: PathBuf,
    modified: SystemTime,
    size: u64,
    prefix: Option<PathBuf>,
}

fn read_npmrc_prefix(path: &Path, home: Option<&Path>) -> Option<PathBuf> {
    static CACHE: Mutex<Option<CachedPrefix>> = Mutex::new(None);
    use std::io::Read;
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_NPMRC_BYTES {
        return None;
    }
    let modified = metadata.modified().ok()?;
    let mut cache = CACHE.lock();
    if let Some(hit) = cache
        .as_ref()
        .filter(|hit| hit.path == path && hit.modified == modified && hit.size == metadata.len())
    {
        return hit.prefix.clone();
    }
    let mut content = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_NPMRC_BYTES + 1)
        .read_to_string(&mut content)
        .ok()?;
    if content.len() as u64 > MAX_NPMRC_BYTES {
        return None;
    }
    let prefix = parse_npmrc_prefix(&content, home);
    *cache = Some(CachedPrefix {
        path: path.to_owned(),
        modified,
        size: metadata.len(),
        prefix: prefix.clone(),
    });
    prefix
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
        last = valid_install_path(&resolved).then_some(resolved);
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
    valid_install_path(&base).then_some(base)
}

#[cfg(test)]
mod tests;
