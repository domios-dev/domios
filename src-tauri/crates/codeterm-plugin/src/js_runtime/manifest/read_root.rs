use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReadRootEnvironment {
    pub variable: String,
    #[serde(default)]
    pub subdir: String,
}

impl ReadRootEnvironment {
    pub fn validate(&self) -> Result<(), String> {
        if self.variable.is_empty() || self.variable.contains(['=', '\0']) {
            return Err("invalid session root environment variable".into());
        }
        if self
            .subdir
            .split(['/', '\\'])
            .any(|part| matches!(part, "." | "..") || part.contains(':'))
            || self.subdir.starts_with(['/', '\\'])
        {
            return Err("session root environment subdir must stay inside its home".into());
        }
        Ok(())
    }

    pub fn resolve(&self, inherited: Option<std::ffi::OsString>) -> Option<PathBuf> {
        self.validate().ok()?;
        let home = PathBuf::from(inherited.filter(|value| !value.is_empty())?);
        Some(
            self.subdir
                .split(['/', '\\'])
                .filter(|part| !part.is_empty())
                .fold(home, |path, part| path.join(part)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_home_diagnostic_manifest_root_is_read_only_and_confined_to_subdir() {
        let config = ReadRootEnvironment {
            variable: "FIXTURE_HOME".into(),
            subdir: "projects".into(),
        };
        assert_eq!(
            config.resolve(Some("foreign".into())),
            Some(PathBuf::from("foreign").join("projects"))
        );
        assert_eq!(config.resolve(Some("".into())), None);
        for subdir in ["../outside", "/outside", "C:\\outside", "..\\outside"] {
            assert!(ReadRootEnvironment {
                subdir: subdir.into(),
                ..config.clone()
            }
            .validate()
            .is_err());
        }
    }
}
