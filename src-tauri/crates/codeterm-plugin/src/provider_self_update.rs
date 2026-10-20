use std::path::{Path, PathBuf};

fn small_text(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSelfUpdateCapability {
    pub npm_package: String,
    pub standalone_directory: String,
    pub standalone_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installation {
    Npm { prefix: PathBuf },
    Standalone,
}

pub fn classify_installation(
    capability: &ProviderSelfUpdateCapability,
    executable: &Path,
    package_root: Option<(&Path, &str)>,
    platform: &str,
) -> Option<Installation> {
    if let Some((root, metadata)) = package_root {
        let package: serde_json::Value = serde_json::from_str(metadata).ok()?;
        if package.get("name").and_then(serde_json::Value::as_str)
            == Some(capability.npm_package.as_str())
            && executable.starts_with(root)
        {
            let mut prefix = root;
            for _ in capability
                .npm_package
                .split('/')
                .chain(std::iter::once("node_modules"))
            {
                prefix = prefix.parent()?;
            }
            if root.strip_prefix(prefix).ok()?
                == Path::new("node_modules").join(&capability.npm_package)
            {
                if platform != "windows" {
                    if !prefix.file_name().is_some_and(|name| name == "lib") {
                        return None;
                    }
                    prefix = prefix.parent()?;
                }
                return Some(Installation::Npm {
                    prefix: prefix.to_owned(),
                });
            }
        }
    }
    let normalized = executable.to_string_lossy().replace('\\', "/");
    let directory = capability.standalone_directory.trim_matches('/');
    (!directory.is_empty() && normalized.contains(&format!("/{directory}/")))
        .then_some(Installation::Standalone)
}

pub fn detect_installation(
    capability: &ProviderSelfUpdateCapability,
    resolved: &Path,
    platform: &str,
) -> Option<Installation> {
    let executable = resolved.canonicalize().ok()?;
    for root in executable.ancestors().take(10) {
        if let Some(metadata) = small_text(&root.join("package.json")) {
            if let Some(installation) =
                classify_installation(capability, &executable, Some((root, &metadata)), platform)
            {
                return Some(installation);
            }
        }
    }
    // Windows npm shims are files rather than symlinks; verify their package target.
    if let Some(parent) = resolved.parent() {
        let root = parent.join("node_modules").join(&capability.npm_package);
        if let (Some(shim), Some(metadata)) =
            (small_text(resolved), small_text(&root.join("package.json")))
        {
            let target = format!("node_modules/{}/", capability.npm_package);
            if shim.replace('\\', "/").contains(&target) {
                return classify_installation(
                    capability,
                    &root.join("bin"),
                    Some((&root, &metadata)),
                    platform,
                );
            }
        }
    }
    classify_installation(capability, &executable, None, platform)
}

pub fn update_command(
    capability: &ProviderSelfUpdateCapability,
    resolved: &Path,
    installation: &Installation,
    platform: &str,
) -> Option<String> {
    let quote = |value: &str| {
        let escaped = value.replace('\'', "'\\''");
        if platform == "windows" {
            format!("'{}'", value.replace('\'', "''"))
        } else {
            format!("'{escaped}'")
        }
    };
    match installation {
        Installation::Npm { prefix } => Some(format!(
            "npm install --global --prefix {} {}",
            quote(&prefix.to_string_lossy()),
            quote(&format!("{}@{{version}}", capability.npm_package)),
        )),
        Installation::Standalone => {
            if capability.standalone_args.is_empty() {
                return None;
            }
            let invocation = std::iter::once(quote(&resolved.to_string_lossy()))
                .chain(capability.standalone_args.iter().map(|arg| quote(arg)))
                .collect::<Vec<_>>()
                .join(" ");
            Some(if platform == "windows" {
                format!("& {invocation}")
            } else {
                invocation
            })
        }
    }
}

#[cfg(test)]
mod tests;
