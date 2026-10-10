use std::path::{Path, PathBuf};

use crate::provider::PreTrustOutcome;

pub(super) fn record_path() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        Some(root) => {
            let root = PathBuf::from(root);
            root.is_absolute().then(|| root.join(".claude.json"))
        }
        None => dirs::home_dir().map(|home| home.join(".claude.json")),
    }
}

pub(crate) fn project_key(path: &Path) -> Result<String, String> {
    let canonical = std::fs::canonicalize(path).map_err(|_| "launch cwd cannot be resolved")?;
    let plain = codeterm_paths::strip_verbatim_prefix(&canonical.to_string_lossy());
    Ok(format_key(&plain, cfg!(windows)))
}

fn format_key(path: &str, windows: bool) -> String {
    if !windows {
        return path.to_string();
    }
    let normalized = path.replace('\\', "/");
    let mut key = if let Some(unc) = normalized.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else {
        normalized
            .strip_prefix("//?/")
            .unwrap_or(&normalized)
            .to_string()
    };
    if key.as_bytes().get(1) == Some(&b':') {
        key.replace_range(..1, &key[..1].to_ascii_uppercase());
    }
    key
}

fn keys(path: &str) -> Result<Vec<String>, String> {
    Ok(vec![
        "projects".into(),
        project_key(Path::new(path))?,
        "hasTrustDialogAccepted".into(),
    ])
}

pub(super) fn grant(path: &str) -> PreTrustOutcome {
    let Some(record) = record_path() else {
        return PreTrustOutcome::Skipped("configuration root unavailable".into());
    };
    let keys = match keys(path) {
        Ok(keys) => keys,
        Err(reason) => return PreTrustOutcome::Skipped(reason),
    };
    crate::json_boolean_record::grant(&record, &keys)
}

pub(super) fn trusted(path: &str) -> Result<bool, String> {
    let Some(record) = record_path() else {
        return Ok(false);
    };
    let Ok(keys) = keys(path) else {
        return Ok(false);
    };
    Ok(crate::json_boolean_record::trusted(&record, &keys).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_keys_match_platform_evidence() {
        assert_eq!(
            format_key("/private/tmp/trustprobe.d1AhFE", false),
            "/private/tmp/trustprobe.d1AhFE"
        );
        assert_eq!(
            format_key(r"\\?\c:\Users\alex\src\demo", true),
            "C:/Users/alex/src/demo"
        );
        assert_eq!(
            format_key(r"\\server\share\project", true),
            "//server/share/project"
        );
    }

    #[cfg(unix)]
    #[test]
    fn project_key_resolves_symlinked_launch_cwd() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        assert_eq!(project_key(&alias), project_key(&real));
        assert_eq!(
            project_key(&real).unwrap(),
            std::fs::canonicalize(&real).unwrap().to_string_lossy()
        );
    }
}
