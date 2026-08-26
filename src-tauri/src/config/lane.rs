use std::path::Path;

pub const MAX_SHARED_DIRECTORIES: usize = 100;
const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_ENTRY_CHARS: usize = 512;
const MAX_SETUP_COMMAND_CHARS: usize = 4096;

/// Repo-declared lane bootstrap: `lane.shared_directories` + `lane.setup_command`
/// from `<repo>/.codeterm/config.toml`. Exactly two keys; hostile or malformed
/// input parses to the empty default, never an error.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RepoLaneConfig {
    pub shared_directories: Vec<String>,
    pub setup_command: Option<String>,
}

impl RepoLaneConfig {
    pub fn is_empty(&self) -> bool {
        self.shared_directories.is_empty() && self.setup_command.is_none()
    }
}

pub fn load_repo_lane_config(repo_root: &Path) -> RepoLaneConfig {
    let path = repo_root.join(".codeterm").join("config.toml");
    let Ok(metadata) = std::fs::metadata(&path) else {
        return RepoLaneConfig::default();
    };
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return RepoLaneConfig::default();
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return RepoLaneConfig::default();
    };
    parse_repo_lane_config(&text)
}

pub fn parse_repo_lane_config(text: &str) -> RepoLaneConfig {
    if text.len() > MAX_CONFIG_BYTES as usize {
        return RepoLaneConfig::default();
    }
    let Ok(value) = text.parse::<toml::Value>() else {
        return RepoLaneConfig::default();
    };
    let Some(lane) = value.get("lane").and_then(toml::Value::as_table) else {
        return RepoLaneConfig::default();
    };
    RepoLaneConfig {
        shared_directories: normalize_shared_directories(lane.get("shared_directories")),
        setup_command: normalize_setup_command(lane.get("setup_command")),
    }
}

fn normalize_setup_command(value: Option<&toml::Value>) -> Option<String> {
    let trimmed = value?.as_str()?.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > MAX_SETUP_COMMAND_CHARS
        || trimmed.chars().any(char::is_control)
    {
        return None;
    }
    Some(trimmed.to_string())
}

/// Port of Orca's `normalizeSharedDirectories` hardening: separator
/// normalization, dedup, rejection of absolute/drive-letter/traversal/`.git`/
/// uncollapsed entries. A list over the 100-entry bound is rejected whole.
fn normalize_shared_directories(value: Option<&toml::Value>) -> Vec<String> {
    let Some(entries) = value.and_then(toml::Value::as_array) else {
        return Vec::new();
    };
    if entries.len() > MAX_SHARED_DIRECTORIES {
        return Vec::new();
    }
    let mut normalized_entries: Vec<String> = Vec::new();
    for entry in entries {
        let Some(raw) = entry.as_str() else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.chars().count() > MAX_ENTRY_CHARS {
            continue;
        }
        let replaced = trimmed.replace('\\', "/");
        let stripped = replaced.strip_prefix("./").unwrap_or(&replaced);
        let normalized = stripped.trim_end_matches('/');
        if normalized.is_empty() || normalized.starts_with('/') || has_drive_prefix(normalized) {
            continue;
        }
        if normalized
            .split('/')
            .any(|segment| matches!(segment, "" | "." | ".." | ".git"))
        {
            continue;
        }
        if !normalized_entries.iter().any(|seen| seen == normalized) {
            normalized_entries.push(normalized.to_string());
        }
    }
    normalized_entries
}

fn has_drive_prefix(path: &str) -> bool {
    let mut chars = path.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(shared: &str, setup: &str) -> String {
        format!("[lane]\nshared_directories = {shared}\nsetup_command = {setup}\n")
    }

    #[test]
    fn lane_bootstrap_config_parses_two_keys() {
        let parsed = parse_repo_lane_config(&config(
            r#"["node_modules", "apps/web/node_modules"]"#,
            r#""npm install""#,
        ));
        assert_eq!(
            parsed.shared_directories,
            vec!["node_modules", "apps/web/node_modules"]
        );
        assert_eq!(parsed.setup_command.as_deref(), Some("npm install"));
    }

    #[test]
    fn lane_bootstrap_config_normalizes_separators_prefixes_and_duplicates() {
        let parsed = parse_repo_lane_config(&config(
            r#"["./cache", "dir\\sub", "trail///", " node_modules ", "node_modules"]"#,
            r#""""#,
        ));
        assert_eq!(
            parsed.shared_directories,
            vec!["cache", "dir/sub", "trail", "node_modules"]
        );
        assert_eq!(parsed.setup_command, None);
    }

    #[test]
    fn lane_bootstrap_config_rejects_traversal_absolute_drive_and_git_entries() {
        let parsed = parse_repo_lane_config(&config(
            r#"["../up", "a/../b", "/abs", "C:/x", "c:\\x", ".git", "a/.git/b", "a//b", "apps/./web", ".", ""]"#,
            r#""npm install""#,
        ));
        assert!(parsed.shared_directories.is_empty());
        assert_eq!(parsed.setup_command.as_deref(), Some("npm install"));
    }

    #[test]
    fn lane_bootstrap_config_rejects_over_100_entries_whole() {
        let entries: Vec<String> = (0..101).map(|index| format!("\"dir{index}\"")).collect();
        let parsed = parse_repo_lane_config(&config(
            &format!("[{}]", entries.join(", ")),
            r#""npm install""#,
        ));
        assert!(parsed.shared_directories.is_empty());
    }

    #[test]
    fn lane_bootstrap_config_accepts_exactly_100_entries() {
        let entries: Vec<String> = (0..100).map(|index| format!("\"dir{index}\"")).collect();
        let parsed =
            parse_repo_lane_config(&config(&format!("[{}]", entries.join(", ")), r#""x""#));
        assert_eq!(parsed.shared_directories.len(), 100);
    }

    #[test]
    fn lane_bootstrap_config_rejects_non_string_entries_and_multiline_setup() {
        let parsed = parse_repo_lane_config(&config(
            r#"[1, true, "node_modules"]"#,
            "\"line one\\nline two\"",
        ));
        assert_eq!(parsed.shared_directories, vec!["node_modules"]);
        assert_eq!(parsed.setup_command, None);
    }

    #[test]
    fn lane_bootstrap_config_rejects_overlong_setup_command_and_entries() {
        let long_entry = "a".repeat(513);
        let long_setup = "b".repeat(4097);
        let parsed = parse_repo_lane_config(&config(
            &format!(r#"["{long_entry}"]"#),
            &format!(r#""{long_setup}""#),
        ));
        assert!(parsed.is_empty());
    }

    #[test]
    fn lane_bootstrap_config_malformed_toml_yields_no_bootstrap() {
        assert!(parse_repo_lane_config("[lane\nnot toml").is_empty());
        assert!(parse_repo_lane_config("lane = 3").is_empty());
        let oversized = format!("# {}", "x".repeat(MAX_CONFIG_BYTES as usize));
        assert!(parse_repo_lane_config(&oversized).is_empty());
    }

    #[test]
    fn lane_bootstrap_config_absent_file_yields_no_bootstrap() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_repo_lane_config(dir.path()).is_empty());
    }

    #[test]
    fn lane_bootstrap_config_loads_from_repo_dot_codeterm() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeterm");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            config(r#"["node_modules"]"#, r#""npm ci""#),
        )
        .unwrap();
        let loaded = load_repo_lane_config(dir.path());
        assert_eq!(loaded.shared_directories, vec!["node_modules"]);
        assert_eq!(loaded.setup_command.as_deref(), Some("npm ci"));
    }

    #[test]
    fn lane_bootstrap_config_oversized_file_yields_no_bootstrap() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeterm");
        std::fs::create_dir_all(&config_dir).unwrap();
        let mut text = config(r#"["node_modules"]"#, r#""npm ci""#);
        text.push_str(&format!("# {}\n", "x".repeat(MAX_CONFIG_BYTES as usize)));
        std::fs::write(config_dir.join("config.toml"), text).unwrap();
        assert!(load_repo_lane_config(dir.path()).is_empty());
    }
}
