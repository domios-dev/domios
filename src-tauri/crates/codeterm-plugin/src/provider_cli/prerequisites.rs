use crate::js_runtime::{probe_bin, BinResolution};

#[cfg(any(windows, test))]
mod powershell;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CommandPrerequisite {
    pub binary: String,
    pub installed: Option<bool>,
    pub hint: String,
}

fn command_binaries(command: &str) -> Vec<String> {
    let tokens: Vec<&str> = command
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '(' | ')' | '{' | '}' | '|' | '&'))
        .map(|token| token.trim_matches(['\'', '"']))
        .filter(|token| !token.is_empty())
        .collect();
    let mut binaries = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let candidate = if *token == "command" && tokens.get(index + 1) == Some(&"-v") {
            tokens.get(index + 2)
        } else if *token == "which" || token.eq_ignore_ascii_case("Get-Command") {
            tokens.get(index + 1)
        } else {
            None
        };
        if let Some(binary) = candidate.filter(|binary| valid_binary(binary)) {
            binaries.push((*binary).to_owned());
        }
    }
    if !binaries.is_empty() || tokens.first().is_some_and(|binary| valid_binary(binary)) {
        for segment in command.split([';', '|', '&', '{', '}', '\n']) {
            let mut words = segment.split_whitespace();
            let mut binary = words.next().unwrap_or_default();
            if matches!(binary, "then" | "else") {
                binary = words.next().unwrap_or_default();
            }
            let binary = binary.trim_matches(['\'', '"']);
            if valid_binary(binary) {
                binaries.push(binary.to_owned());
            }
        }
    }
    binaries.sort();
    binaries.dedup();
    binaries
}

fn valid_binary(binary: &str) -> bool {
    !binary.is_empty()
        && !matches!(
            binary,
            "if" | "then"
                | "else"
                | "fi"
                | "elif"
                | "do"
                | "done"
                | "for"
                | "while"
                | "try"
                | "catch"
                | "which"
                | "Get-Command"
                | "export"
                | "exec"
                | "command"
                | "echo"
                | "printf"
                | "cd"
                | "test"
                | "true"
                | "false"
                | "exit"
                | "return"
                | "throw"
                | "Write-Output"
                | "Write-Error"
                | "irm"
                | "iex"
                | "iwr"
        )
        && !is_powershell_cmdlet(binary)
        && binary
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !binary.starts_with('-')
        && binary
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '_' | '.'))
}

/// PowerShell Verb-Noun cmdlets (Invoke-RestMethod, Set-ExecutionPolicy) are shell built-ins, not binaries.
fn is_powershell_cmdlet(token: &str) -> bool {
    let mut parts = token.splitn(2, '-');
    let (Some(verb), Some(noun)) = (parts.next(), parts.next()) else {
        return false;
    };
    let capitalized = |s: &str| s.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    capitalized(verb) && capitalized(noun) && verb.chars().all(|c| c.is_ascii_alphabetic())
}

fn setup_hint(binary: &str) -> String {
    let source = match binary {
        "uv" => "https://docs.astral.sh/uv/getting-started/installation/",
        "pipx" => "https://pipx.pypa.io/stable/installation/",
        _ => "its official setup instructions or your OS package manager",
    };
    format!("Install {binary} using {source}, then rerun the manifest command.")
}

pub(super) fn observe(command: Option<&str>) -> Vec<CommandPrerequisite> {
    let prerequisites = command
        .into_iter()
        .flat_map(command_binaries)
        .map(|binary| {
            let installed = match probe_bin(&binary).resolution {
                BinResolution::Found(_) => Some(true),
                BinResolution::Missing => Some(false),
                BinResolution::Unknown => None,
            };
            CommandPrerequisite {
                hint: setup_hint(&binary),
                binary,
                installed,
            }
        })
        .collect::<Vec<_>>();
    #[cfg(windows)]
    {
        let mut prerequisites = prerequisites;
        if let Some(command) = command.filter(|command| powershell::runs_downloaded_script(command))
        {
            let values = codeterm_util::windows_powershell::read_execution_policy_values();
            if let Some(prerequisite) = powershell::prerequisite(command, &values) {
                prerequisites.push(prerequisite);
            }
        }
        prerequisites
    }
    #[cfg(not(windows))]
    prerequisites
}

pub fn missing_prerequisite_hint(prerequisites: &[CommandPrerequisite]) -> Option<String> {
    let missing: Vec<_> = prerequisites
        .iter()
        .filter(|item| item.installed == Some(false))
        .collect();
    if missing.is_empty() {
        return None;
    }
    Some(format!("Missing command prerequisites: {}. {} Conditional installers may use an available alternative.",
        missing.iter().map(|item| item.binary.as_str()).collect::<Vec<_>>().join(", "),
        missing.iter().map(|item| item.hint.as_str()).collect::<Vec<_>>().join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_commands_supply_runner_and_alternative_checks() {
        assert_eq!(
            command_binaries("uv tool install opaque-package"),
            vec!["uv"]
        );
        assert_eq!(command_binaries("if command -v uv; then uv tool install x; elif command -v pipx; then pipx install x; fi"), vec!["pipx", "uv"]);
        assert_eq!(
            command_binaries(
                "if command -v uv >/dev/null 2>&1; then uv tool install x; else pipx install x; fi"
            ),
            vec!["pipx", "uv"]
        );
        assert_eq!(
            command_binaries("uv tool install x || pipx install x"),
            vec!["pipx", "uv"]
        );
        assert_eq!(
            command_binaries(
                "if (Get-Command 'uv' -ErrorAction SilentlyContinue) { uv tool install x }"
            ),
            vec!["uv"]
        );
        assert_eq!(
            command_binaries("if unknown-script; then something; fi"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn powershell_builtins_are_not_prerequisites() {
        assert_eq!(
            command_binaries("irm https://x.ai/cli/install.ps1 | iex"),
            Vec::<String>::new()
        );
        assert_eq!(
            command_binaries("$s = Join-Path $env:TEMP 'a.ps1'; Invoke-WebRequest -Uri 'https://e/x' -OutFile $s; & $s"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn missing_prerequisites_are_named_with_setup_guidance_without_guessing_unknown() {
        let mut prerequisites = vec![
            CommandPrerequisite {
                binary: "uv".into(),
                installed: Some(false),
                hint: setup_hint("uv"),
            },
            CommandPrerequisite {
                binary: "pipx".into(),
                installed: Some(false),
                hint: setup_hint("pipx"),
            },
        ];
        let hint = missing_prerequisite_hint(&prerequisites).unwrap();
        assert!(hint.contains("uv, pipx"));
        assert!(hint.contains("https://docs.astral.sh/uv/"));
        assert!(hint.contains("https://pipx.pypa.io/"));
        prerequisites[0].installed = Some(true);
        prerequisites[1].installed = None;
        assert_eq!(missing_prerequisite_hint(&prerequisites), None);
    }
}
