use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Eligible,
    SkipProtected,
    SkipNoCwd,
    SkipMissingCwd,
    SkipDisabled,
}

impl Verdict {
    pub fn is_eligible(self) -> bool {
        matches!(self, Verdict::Eligible)
    }
}

pub fn evaluate(protected: bool, cwd: Option<&Path>, indexing_enabled: bool) -> Verdict {
    if protected {
        return Verdict::SkipProtected;
    }
    let Some(cwd) = cwd else { return Verdict::SkipNoCwd };
    if !cwd.exists() {
        return Verdict::SkipMissingCwd;
    }
    if !indexing_enabled {
        return Verdict::SkipDisabled;
    }
    Verdict::Eligible
}

pub fn ext_is_indexable(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "rs" | "ts" | "tsx" | "mts" | "cts" | "js" | "mjs" | "cjs" | "jsx"
            | "py" | "pyi" | "go" | "java"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn matrix() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().to_path_buf();
        let absent = PathBuf::from("/definitely/does/not/exist/9b8a8a3");

        assert_eq!(evaluate(true, Some(&real), true), Verdict::SkipProtected);
        assert_eq!(evaluate(false, None, true), Verdict::SkipNoCwd);
        assert_eq!(evaluate(false, Some(&absent), true), Verdict::SkipMissingCwd);
        assert_eq!(evaluate(false, Some(&real), false), Verdict::SkipDisabled);
        assert_eq!(evaluate(false, Some(&real), true), Verdict::Eligible);
    }

    #[test]
    fn ext_filter() {
        assert!(ext_is_indexable("rs"));
        assert!(ext_is_indexable("TSX"));
        assert!(!ext_is_indexable("md"));
        assert!(!ext_is_indexable(""));
    }
}
