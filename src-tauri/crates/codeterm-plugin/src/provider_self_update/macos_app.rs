use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacosAppBundle {
    pub bundle_identifier: String,
    pub team_identifier: String,
    pub executable_paths: Vec<String>,
    pub update_guidance: String,
}

impl MacosAppBundle {
    pub(super) fn enclosing_app<'a>(&self, executable: &'a Path) -> Option<&'a Path> {
        executable.ancestors().skip(1).find(|app| {
            app.extension().is_some_and(|ext| ext == "app")
                && executable.strip_prefix(app).is_ok_and(|relative| {
                    relative.starts_with("Contents")
                        && self
                            .executable_paths
                            .iter()
                            .any(|path| relative == Path::new(path))
                })
        })
    }

    #[cfg(any(target_os = "macos", test))]
    pub(super) fn signing_requirement(&self) -> Option<String> {
        let bundle = &self.bundle_identifier;
        let team = &self.team_identifier;
        if bundle.is_empty()
            || !bundle
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
            || team.len() != 10
            || !team
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            || self.update_guidance.trim().is_empty()
        {
            return None;
        }
        Some(format!(
            "identifier \"{bundle}\" and anchor apple generic and certificate leaf[subject.OU] = \"{team}\""
        ))
    }
}

#[cfg(target_os = "macos")]
pub(super) fn verify_signature(app: &Path, owner: &MacosAppBundle) -> bool {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    use wait_timeout::ChildExt;

    let Some(requirement) = owner.signing_requirement() else {
        return false;
    };
    let Ok(mut child) = Command::new("/usr/bin/codesign")
        .env_clear()
        .args(["--verify", "--deep", "--strict", "--all-architectures"])
        .arg(format!("-R={requirement}"))
        .arg(app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Ok(Some(status)) = child.wait_timeout(Duration::from_secs(15)) {
        return status.success();
    }
    let _ = child.kill();
    let _ = child.wait();
    false
}

#[cfg(not(target_os = "macos"))]
pub(super) fn verify_signature(_: &Path, _: &MacosAppBundle) -> bool {
    false
}
