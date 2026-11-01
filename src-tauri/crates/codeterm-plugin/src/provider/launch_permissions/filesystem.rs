use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScopedFilesystemPermissions {
    pub no_prompt_args: Vec<String>,
    pub read_all_rules: Vec<String>,
    pub read_root_rule: String,
    pub write_root_rule: String,
    pub additional_directory_args: Vec<String>,
}

impl LaunchPermissionCapability {
    pub fn scoped_filesystem_args(
        &self,
        read_all: bool,
        read_roots: &[String],
        write_roots: &[String],
    ) -> Result<Vec<String>, String> {
        let policy = self
            .scoped_filesystem
            .as_ref()
            .ok_or("Scoped filesystem permissions are unsupported")?;
        if policy.no_prompt_args.is_empty() || self.allow_rule_args_format.is_empty() {
            return Err(
                "Scoped filesystem permissions must declare non-prompting mode and allow rules"
                    .into(),
            );
        }
        let mut args = policy.no_prompt_args.clone();
        let mut rules = if read_all {
            policy.read_all_rules.clone()
        } else {
            Vec::new()
        };
        for (roots, template) in [
            (read_roots, &policy.read_root_rule),
            (write_roots, &policy.write_root_rule),
        ] {
            for root in roots {
                let path = absolute_rule_path(root)?;
                rules.push(template.replace("{path}", &path));
            }
        }
        for rule in rules {
            args.extend(
                self.allow_rule_args_format
                    .iter()
                    .map(|arg| arg.replace("{rule}", &rule)),
            );
        }
        for root in write_roots {
            args.extend(
                policy
                    .additional_directory_args
                    .iter()
                    .map(|arg| arg.replace("{path}", root)),
            );
        }
        Ok(args)
    }
}

fn absolute_rule_path(path: &str) -> Result<String, String> {
    if path.starts_with('/') && path.contains('\\') {
        return Err("Scoped filesystem root cannot change path separators".into());
    }
    let path = path.replace('\\', "/");
    if path
        .chars()
        .any(|ch| ch.is_control() || "*?[]{}!,".contains(ch))
        || path.starts_with("//")
    {
        return Err(
            "Scoped filesystem root cannot be represented without widening its permission rule"
                .into(),
        );
    }
    let path = if path.as_bytes().get(1) == Some(&b':')
        && path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
    {
        format!(
            "/{}/{}",
            path[..1].to_ascii_lowercase(),
            path.get(3..)
                .filter(|_| path.as_bytes().get(2) == Some(&b'/'))
                .ok_or("Scoped filesystem root must be absolute")?
        )
    } else if path.starts_with('/') {
        path
    } else {
        return Err("Scoped filesystem root must be absolute".into());
    };
    if path.split('/').any(|part| part == "." || part == "..") {
        return Err("Scoped filesystem root must be canonical".into());
    }
    Ok(format!("/{}", path.trim_end_matches('/')))
}

#[cfg(test)]
mod tests;
