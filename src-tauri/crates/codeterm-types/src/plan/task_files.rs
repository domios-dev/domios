use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskFiles {
    #[serde(default)]
    pub edit: Vec<String>,
    #[serde(default)]
    pub read: Vec<String>,
}

impl TaskFiles {
    pub fn validated(mut self) -> Result<Self, String> {
        if self.edit.len() + self.read.len() > 256 {
            return Err("task files exceed 256 entries".into());
        }
        for paths in [&mut self.edit, &mut self.read] {
            let mut seen = BTreeSet::new();
            for path in paths.iter() {
                if path.trim().is_empty() || path.len() > 4096 || path.chars().any(char::is_control)
                {
                    return Err("task file paths must be nonempty, bounded and contain no control characters".into());
                }
            }
            paths.retain(|path| seen.insert(path.clone()));
        }
        if self.edit.iter().any(|path| self.read.contains(path)) {
            return Err("task files cannot be both editable and read-only".into());
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_files_validate_deduplicate_and_preserve_new_edit_paths() {
        let files = TaskFiles {
            edit: vec!["new.rs".into(), "new.rs".into()],
            read: vec!["context.md".into()],
        }
        .validated()
        .unwrap();
        assert_eq!(files.edit, ["new.rs"]);
        assert_eq!(files.read, ["context.md"]);
    }
    #[test]
    fn task_files_refuse_invalid_and_conflicting_paths() {
        for path in ["", " ", "bad\npath"] {
            assert!(TaskFiles {
                edit: vec![path.into()],
                ..Default::default()
            }
            .validated()
            .is_err());
        }
        assert!(TaskFiles {
            edit: vec!["x".into()],
            read: vec!["x".into()]
        }
        .validated()
        .is_err());
        assert!(serde_json::from_str::<TaskFiles>(r#"{"write":["x"]}"#).is_err());
    }
}
