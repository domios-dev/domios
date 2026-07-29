use std::path::{Path, PathBuf};

pub const KEEP: &str = "keep";
pub const DISCARD: &str = "discard";
pub const SUPERSEDED: &str = "superseded";

pub fn valid_verdict(verdict: &str) -> bool {
    matches!(verdict, KEEP | DISCARD | SUPERSEDED)
}

pub fn slugify(raw: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

pub fn skills_root(plan_dir: &Path) -> PathBuf {
    plan_dir.join("skills")
}

pub fn skill_dir(plan_dir: &Path, slug: &str) -> PathBuf {
    skills_root(plan_dir).join(slug)
}

pub fn skill_md_path(plan_dir: &Path, slug: &str) -> PathBuf {
    skill_dir(plan_dir, slug).join("SKILL.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_normalizes() {
        assert_eq!(slugify("Deploy To Staging"), "deploy-to-staging");
        assert_eq!(slugify("  weird__name!! "), "weird-name");
        assert_eq!(slugify("already-kebab"), "already-kebab");
        assert_eq!(slugify("MixedCASE123"), "mixedcase123");
    }

    #[test]
    fn verdict_validation() {
        assert!(valid_verdict("keep"));
        assert!(valid_verdict("discard"));
        assert!(valid_verdict("superseded"));
        assert!(!valid_verdict("maybe"));
    }
}
