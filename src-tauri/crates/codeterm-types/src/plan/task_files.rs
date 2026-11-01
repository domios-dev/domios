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
    pub fn shared_edit_paths(&self) -> Result<Vec<String>, String> {
        if self.edit.is_empty() {
            return Err("shared placement requires declared edit files".into());
        }
        if self.read.iter().any(|path| self.owns_edit_path(path)) {
            return Err("shared edit prefixes cannot contain declared read-only files".into());
        }
        self.edit
            .iter()
            .map(|path| normalize_edit_path(path))
            .collect()
    }

    pub fn owns_edit_path(&self, path: &str) -> bool {
        let Ok(path) = normalize_edit_path(path) else {
            return false;
        };
        self.edit
            .iter()
            .filter_map(|edit| normalize_edit_path(edit).ok())
            .any(|edit| {
                path == edit
                    || path
                        .strip_prefix(&edit)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
    }

    pub fn overlapping_edit_path<'a>(&'a self, other: &Self) -> Option<&'a str> {
        self.edit
            .iter()
            .find(|path| {
                other.owns_edit_path(path)
                    || other.edit.iter().any(|owned| {
                        let Ok(path) = normalize_edit_path(path) else {
                            return false;
                        };
                        let Ok(owned) = normalize_edit_path(owned) else {
                            return false;
                        };
                        owned
                            .strip_prefix(&path)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                    })
            })
            .map(String::as_str)
    }

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

pub fn normalize_edit_path(path: &str) -> Result<String, String> {
    let path = path.replace('\\', "/");
    let path = path.trim_end_matches('/');
    if path.is_empty()
        || path.contains(':')
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| {
            part.is_empty()
                || part.ends_with('.')
                || part.ends_with(' ')
                || part.eq_ignore_ascii_case(".git")
        })
    {
        return Err(format!(
            "edit path '{path}' must be repository-relative without traversal"
        ));
    }
    Ok(if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    })
}

#[cfg(test)]
mod tests;
