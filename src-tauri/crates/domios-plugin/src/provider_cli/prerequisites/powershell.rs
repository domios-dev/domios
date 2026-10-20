use super::CommandPrerequisite;
use codeterm_util::windows_powershell::ExecutionPolicyValues;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutionPolicy {
    Restricted,
    AllSigned,
    RemoteSigned,
    Unrestricted,
    Bypass,
    Undefined,
}

impl ExecutionPolicy {
    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "restricted" => Self::Restricted,
            "allsigned" => Self::AllSigned,
            "remotesigned" => Self::RemoteSigned,
            "unrestricted" => Self::Unrestricted,
            "bypass" => Self::Bypass,
            _ => Self::Undefined,
        }
    }

    fn blocks_unsigned(self) -> bool {
        matches!(self, Self::Restricted | Self::AllSigned | Self::Undefined)
    }
}

fn effective_policy(values: &ExecutionPolicyValues) -> (ExecutionPolicy, bool) {
    for (index, value) in values.policies.iter().enumerate() {
        let policy = if index < 2 && values.enable_scripts[index] == Some(0) {
            ExecutionPolicy::Restricted
        } else {
            value
                .as_deref()
                .map(ExecutionPolicy::parse)
                .unwrap_or(ExecutionPolicy::Undefined)
        };
        if policy != ExecutionPolicy::Undefined {
            return (policy, index < 2);
        }
    }
    (ExecutionPolicy::Restricted, false)
}

pub(super) fn runs_downloaded_script(command: &str) -> bool {
    static DOWNLOAD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:Invoke-WebRequest|iwr|Invoke-RestMethod|irm)\b").unwrap()
    });
    static OUT_FILE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)-OutFile\s+(\$[\w:]+|'[^']+'|"[^"]+"|[^\s;|}]+)"#).unwrap()
    });
    if !command.to_ascii_lowercase().contains(".ps1") || !DOWNLOAD.is_match(command) {
        return false;
    }
    OUT_FILE.captures_iter(command).any(|capture| {
        let target = regex::escape(&capture[1]);
        let invocation = Regex::new(&format!(
            r"(?i)(?:&\s*|(?:^|[;{{}}\s])\.\s+|-File\s+){target}(?:$|[\s;}}])"
        ))
        .unwrap();
        invocation.is_match(command)
    })
}

pub(super) fn prerequisite(
    command: &str,
    values: &ExecutionPolicyValues,
) -> Option<CommandPrerequisite> {
    let (policy, group_policy) = effective_policy(values);
    if !runs_downloaded_script(command) || !policy.blocks_unsigned() {
        return None;
    }
    let management = if group_policy {
        " A Group Policy setting controls this policy and must be changed by an administrator."
    } else {
        " If a Group Policy setting is present, it must be changed by an administrator."
    };
    Some(CommandPrerequisite {
        binary: "PowerShell script execution".into(),
        installed: Some(false),
        hint: format!(
            "Unsigned PowerShell scripts are disabled on this PC (effective policy: {policy:?}). To allow them, run Set-ExecutionPolicy -Scope CurrentUser RemoteSigned in Windows PowerShell, then Re-check.{management}"
        ),
    })
}

#[cfg(test)]
mod tests;
