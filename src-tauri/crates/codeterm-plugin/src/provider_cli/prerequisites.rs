use crate::js_runtime::{probe_bin_with_extra_dirs, BinResolution};

#[cfg(any(windows, test))]
mod powershell;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, ts_rs::TS)]
pub struct CommandPrerequisite {
    pub binary: String,
    pub installed: Option<bool>,
    pub hint: String,
}

fn command_binaries(command: &str) -> Vec<String> {
    if let Some(else_body) = powershell_else_branch(command) {
        if else_has_actionable_builtin(else_body) && segment_binaries(else_body).is_empty() {
            return Vec::new();
        }
    }
    extract_binaries(command)
}

fn extract_binaries(command: &str) -> Vec<String> {
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
        let words = if matches!(words.first(), Some(&"if" | &"elif" | &"elseif")) {
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
        binaries.extend(segment_binaries_from(&segments));
    }
    binaries.sort();
    binaries.dedup();
    binaries
}

fn segment_binaries(command: &str) -> Vec<String> {
    let segments = command_segments(command);
    let mut binaries = segment_binaries_from(&segments);
    binaries.sort();
    binaries.dedup();
    binaries
}

fn segment_binaries_from(segments: &[&str]) -> Vec<String> {
    let mut binaries = Vec::new();
    for segment in segments {
        let mut words = segment.split_whitespace();
        let mut binary = words.next().unwrap_or_default();
        if matches!(binary, "then" | "else" | "try" | "catch" | "finally") {
            binary = words.next().unwrap_or_default();
        }
        let binary = binary.trim_matches(['\'', '"']);
        if valid_binary(binary) {
            binaries.push(binary.to_owned());
        }
    }
    binaries
}

/// Extracts the top-level `else { ... }` body of a PowerShell `if (...) { ... } else { ... }` installer.
fn powershell_else_branch(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    if trimmed.len() < 2 || !trimmed[..2].eq_ignore_ascii_case("if") {
        return None;
    }
    let mut index = 2;
    if !trimmed[index..].starts_with(|c: char| c.is_ascii_whitespace() || c == '(') {
        return None;
    }
    let skip_ws = |s: &str, pos: &mut usize| {
        while let Some(ch) = s[*pos..].chars().next() {
            if ch.is_ascii_whitespace() {
                *pos += ch.len_utf8();
            } else {
                break;
            }
        }
    };
    let skip_balanced =
        |s: &str, pos: &mut usize, open: char, close: char| -> Option<(usize, usize)> {
            if !s[*pos..].starts_with(open) {
                return None;
            }
            let body_start = *pos + open.len_utf8();
            let mut depth = 0usize;
            let mut quote: Option<char> = None;
            for (offset, ch) in s[*pos..].char_indices() {
                if let Some(delim) = quote {
                    if ch == delim {
                        quote = None;
                    }
                    continue;
                }
                if matches!(ch, '\'' | '"') {
                    quote = Some(ch);
                    continue;
                }
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        let body_end = *pos + offset;
                        *pos = body_end + close.len_utf8();
                        return Some((body_start, body_end));
                    }
                }
            }
            None
        };
    loop {
        skip_ws(trimmed, &mut index);
        skip_balanced(trimmed, &mut index, '(', ')')?;
        skip_ws(trimmed, &mut index);
        skip_balanced(trimmed, &mut index, '{', '}')?;
        skip_ws(trimmed, &mut index);
        let rest = &trimmed[index..];
        if rest.len() >= 6
            && rest[..6].eq_ignore_ascii_case("elseif")
            && rest[6..].starts_with(|c: char| c.is_ascii_whitespace() || c == '(')
        {
            index += 6;
            continue;
        }
        if rest.len() >= 4
            && rest[..4].eq_ignore_ascii_case("else")
            && rest[4..].starts_with(|c: char| c.is_ascii_whitespace() || c == '{')
        {
            index += 4;
            skip_ws(trimmed, &mut index);
            let (body_start, body_end) = skip_balanced(trimmed, &mut index, '{', '}')?;
            skip_ws(trimmed, &mut index);
            return (index == trimmed.len()).then_some(&trimmed[body_start..body_end]);
        }
        return None;
    }
}

fn else_has_actionable_builtin(body: &str) -> bool {
    for segment in command_segments(body) {
        let mut words = segment.split_whitespace();
        let Some(first) = words.next() else {
            continue;
        };
        let token = first.trim_matches(['\'', '"', '(', ')']);
        if matches!(token.to_ascii_lowercase().as_str(), "irm" | "iex" | "iwr") {
            return true;
        }
        if is_powershell_cmdlet(token)
            && !matches!(
                token.to_ascii_lowercase().as_str(),
                "write-error" | "write-output" | "write-host" | "write-warning"
            )
        {
            return true;
        }
        // Handle assignments like `$pkg = Invoke-RestMethod ...`
        if token.starts_with('$')
            && words.any(|word| {
                let candidate = word.trim_matches(['\'', '"', '(', ')']);
                matches!(
                    candidate.to_ascii_lowercase().as_str(),
                    "irm" | "iex" | "iwr"
                ) || (is_powershell_cmdlet(candidate)
                    && !matches!(
                        candidate.to_ascii_lowercase().as_str(),
                        "write-error" | "write-output" | "write-host" | "write-warning"
                    ))
            })
        {
            return true;
        }
    }
    false
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
                | "elseif"
                | "do"
                | "done"
                | "for"
                | "while"
                | "try"
                | "catch"
                | "finally"
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
        "npm" | "npx" | "node" => "https://nodejs.org/en/download",
        "uv" => "https://docs.astral.sh/uv/getting-started/installation/",
        "pipx" => "https://pipx.pypa.io/stable/installation/",
        "bun" => "https://bun.sh/docs/installation",
        "pnpm" => "https://pnpm.io/installation",
        "yarn" => "https://yarnpkg.com/getting-started/install",
        "brew" => "https://brew.sh",
        "curl" => "https://curl.se/download.html",
        "winget" => "https://learn.microsoft.com/windows/package-manager/winget/",
        "scoop" => "https://scoop.sh",
        "choco" => "https://chocolatey.org/install",
        "cargo" => "https://rustup.rs",
        "git" => "https://git-scm.com/downloads",
        _ => "its official setup instructions or your OS package manager",
    };
    format!("Install {binary} using {source}, then rerun the manifest command.")
}

pub(super) fn observe(command: Option<&str>) -> Vec<CommandPrerequisite> {
    let prerequisites = command
        .into_iter()
        .flat_map(command_binaries)
        .map(|binary| {
            let installed = match probe_bin_with_extra_dirs(&binary, &[]).resolution {
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
