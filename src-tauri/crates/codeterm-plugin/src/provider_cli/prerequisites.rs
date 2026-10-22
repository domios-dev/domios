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
    let segments = command_segments(command);
    let tokens: Vec<&str> = segments
        .iter()
        .flat_map(|segment| segment.split_whitespace())
        .map(|token| token.trim_matches(['\'', '"', '(', ')']))
        .filter(|token| !token.is_empty())
        .collect();
    let mut binaries = Vec::new();
    for segment in &segments {
        let words: Vec<_> = segment
            .split_whitespace()
            .map(|word| word.trim_matches(['\'', '"', '(', ')']))
            .collect();
        let words = if matches!(words.first(), Some(&"if" | &"elif")) {
            &words[1..]
        } else {
            &words[..]
        };
        let candidate = match words.first().copied() {
            Some("command") if words.get(1) == Some(&"-v") => words.get(2),
            Some("which") => words.get(1),
            Some(token) if token.eq_ignore_ascii_case("Get-Command") => words.get(1),
            _ => None,
        };
        if let Some(binary) = candidate.filter(|binary| valid_binary(binary)) {
            binaries.push((*binary).to_owned());
        }
    }
    if !binaries.is_empty() || tokens.first().is_some_and(|binary| valid_binary(binary)) {
        for segment in segments {
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

fn command_segments(command: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut chars = command.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if let Some(delimiter) = quote {
            if ch == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            continue;
        }
        if ch == '{' {
            if let Some(end) = command[index + 1..].find('}') {
                let name = &command[index + 1..index + 1 + end];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    while chars
                        .peek()
                        .is_some_and(|(offset, _)| *offset <= index + end + 1)
                    {
                        chars.next();
                    }
                    continue;
                }
            }
        }
        if matches!(ch, ';' | '|' | '&' | '{' | '}' | '\n') {
            segments.push(&command[start..index]);
            start = index + ch.len_utf8();
        }
    }
    segments.push(&command[start..]);
    segments
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
mod tests;
