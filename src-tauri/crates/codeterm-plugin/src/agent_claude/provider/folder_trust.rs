//! Claude Code's folder-trust dialog, recognized from the screen alone.
//! Mirrored by `plugins/provider-claude/src/folderTrust.ts`; parity-tested.

use crate::directory_trust::DirectoryTrustPromptResponse;

const FRAME: &[char] = &[
    '│', '┃', '║', '╭', '╮', '╰', '╯', '┌', '┐', '└', '┘', '╔', '╗', '╚', '╝', '─', '━', '═',
];

/// `None` when no trust dialog is shown; `Hold` for a trust dialog whose shape
/// does not prove which key advances it toward trusting the folder.
pub(super) fn response(screen: &str) -> Option<DirectoryTrustPromptResponse> {
    let rows: Vec<&str> = screen.split('\n').map(content).collect();
    if let Some(path) = rows.iter().position(|row| is_path_row(row)) {
        let trust = (path + 1..rows.len())
            .find(|&i| rows[i].to_lowercase().contains("trust"))?;
        (trust + 1..rows.len()).find(|&i| pointer_label(rows[i]).is_some())?;
        return Some(
            answer(&rows, Some(path), trust).unwrap_or(DirectoryTrustPromptResponse::Hold),
        );
    }
    let trust = rows.iter().position(|row| is_pathless_trust_header(row))?;
    (trust + 1..rows.len()).find(|&i| pointer_label(rows[i]).is_some())?;
    Some(answer(&rows, None, trust).unwrap_or(DirectoryTrustPromptResponse::Hold))
}

struct OptionRow<'a> {
    focused: bool,
    label: &'a str,
}

struct TrustPrompt<'a> {
    question: &'a str,
    options: Vec<OptionRow<'a>>,
    focused: usize,
}

fn answer(
    rows: &[&str],
    path: Option<usize>,
    trust: usize,
) -> Option<DirectoryTrustPromptResponse> {
    if let Some(path) = path {
        if !rows[..path].iter().any(|row| row.ends_with(':')) {
            return None;
        }
    }
    if path.is_none() && !is_pathless_trust_header(rows[trust]) {
        return None;
    }
    let options = choice_rows(rows, trust)?;
    if options.len() != 2 {
        return None;
    }
    if options.iter().filter(|option| option.focused).count() != 1 {
        return None;
    }
    let (yes, _) = trust_choice_indices(&options)?;
    let focused = options.iter().position(|option| option.focused)?;
    let step = if focused == yes {
        b"\r".to_vec()
    } else if yes > focused {
        b"j".to_vec()
    } else {
        b"k".to_vec()
    };
    Some(DirectoryTrustPromptResponse::Answer { step })
}

pub(super) fn parse_prompt(screen: &str) -> Option<(String, Vec<(String, String)>)> {
    let rows: Vec<&str> = screen.split('\n').map(content).collect();
    let prompt = current_prompt(&rows)?;
    Some((
        prompt.question.trim().to_string(),
        prompt
            .options
            .into_iter()
            .enumerate()
            .map(|(index, option)| (option.label.to_string(), (index + 1).to_string()))
            .collect(),
    ))
}

pub(super) fn current_option_index(screen: &str) -> Option<usize> {
    let rows: Vec<&str> = screen.split('\n').map(content).collect();
    current_prompt(&rows).map(|prompt| prompt.focused)
}

fn current_prompt<'a>(rows: &[&'a str]) -> Option<TrustPrompt<'a>> {
    if rows.iter().any(|row| is_path_row(row)) {
        return None;
    }
    let trust = rows.iter().position(|row| is_pathless_trust_header(row))?;
    let options = choice_rows(rows, trust)?;
    if options.len() != 2 {
        return None;
    }
    if options.iter().filter(|option| option.focused).count() != 1 {
        return None;
    }
    let focused = options.iter().position(|option| option.focused)?;
    trust_choice_indices(&options)?;
    Some(TrustPrompt {
        question: rows[trust],
        options,
        focused,
    })
}

fn choice_rows<'a>(rows: &[&'a str], trust: usize) -> Option<Vec<OptionRow<'a>>> {
    let footer = rows.iter().rposition(|row| !row.is_empty())?;
    let footer_text = rows[footer].to_lowercase();
    if !footer_text.contains("enter") || !footer_text.contains("esc") {
        return None;
    }
    let start = (trust + 1..footer)
        .find(|&index| is_trust_option(option_row(rows[index]).label))?;
    let mut options = Vec::new();
    for row in &rows[start..footer] {
        if row.is_empty() {
            continue;
        }
        let option = option_row(row);
        if !is_trust_option(option.label) {
            return None;
        }
        options.push(option);
    }
    Some(options)
}

fn is_pathless_trust_header(row: &str) -> bool {
    let Some((header, question)) = row.split_once(':') else {
        return false;
    };
    !header.trim().is_empty()
        && question.to_lowercase().contains("trust")
        && question.contains('?')
}

fn trust_choice_indices(options: &[OptionRow<'_>]) -> Option<(usize, usize)> {
    let yes = options
        .iter()
        .position(|option| leads_with(option.label, "yes"))?;
    let no = options
        .iter()
        .position(|option| leads_with(option.label, "no"))?;
    options[no]
        .label
        .to_lowercase()
        .contains("exit")
        .then_some((yes, no))
}

fn is_trust_option(label: &str) -> bool {
    leads_with(label, "yes") || leads_with(label, "no")
}

fn content(line: &str) -> &str {
    line.trim_matches(|c: char| c.is_whitespace() || FRAME.contains(&c))
}

fn is_path_row(row: &str) -> bool {
    let bytes = row.as_bytes();
    (bytes.len() > 1 && bytes[0] == b'/' && bytes[1] != b' ')
        || row.starts_with("~/")
        || row.starts_with("~\\")
        || row.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/'))
}

fn pointer_label(row: &str) -> Option<&str> {
    let rest = row.strip_prefix('❯').or_else(|| row.strip_prefix('>'))?;
    rest.starts_with(char::is_whitespace)
        .then(|| rest.trim_start())
}

fn option_row(row: &str) -> OptionRow<'_> {
    let (focused, label) = match pointer_label(row) {
        Some(label) => (true, label),
        None => (false, row),
    };
    OptionRow {
        focused,
        label: strip_index(label),
    }
}

fn strip_index(label: &str) -> &str {
    let digits = label.bytes().take_while(u8::is_ascii_digit).count();
    match label[digits..].strip_prefix('.') {
        Some(after) if digits > 0 && after.starts_with(char::is_whitespace) => after.trim_start(),
        _ => label,
    }
}

fn leads_with(label: &str, word: &str) -> bool {
    label
        .to_ascii_lowercase()
        .strip_prefix(word)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([',', ' ', '.']))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../plugins/provider-claude/tests/fixtures/folder-trust")
    }

    fn expected(expect: &str) -> Option<DirectoryTrustPromptResponse> {
        let answer = |step: &[u8]| {
            Some(DirectoryTrustPromptResponse::Answer {
                step: step.to_vec(),
            })
        };
        match expect {
            "enter" => answer(b"\r"),
            "move-down" => answer(b"j"),
            "move-up" => answer(b"k"),
            "hold" => Some(DirectoryTrustPromptResponse::Hold),
            "none" => None,
            other => panic!("unknown folder-trust expectation {other}"),
        }
    }

    #[test]
    fn shared_fixtures_meet_their_declared_expectations() {
        let cases: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(fixtures().join("cases.json")).expect("cases.json"),
        )
        .expect("cases.json parses");
        let cases = cases.as_array().expect("case list");
        assert!(cases.len() >= 12, "every declared fixture is exercised");
        for case in cases {
            let file = case["file"].as_str().expect("file");
            let screen = std::fs::read_to_string(fixtures().join(file)).expect("fixture");
            assert_eq!(
                response(&screen),
                expected(case["expect"].as_str().expect("expect")),
                "{file}"
            );
        }
    }

    #[test]
    fn enter_is_only_sent_while_the_affirmative_row_is_focused() {
        let screen = "Accessing workspace:\n\n/srv/lane\n\none you trust?\n\n❯ No, exit\n  Yes, I trust this folder\n\nEnter to confirm · Esc to cancel";
        assert_eq!(
            response(screen),
            Some(DirectoryTrustPromptResponse::Answer {
                step: b"j".to_vec()
            })
        );
        let moved = screen
            .replace("❯ No, exit", "  No, exit")
            .replace("  Yes, I trust", "❯ Yes, I trust");
        assert_eq!(
            response(&moved),
            Some(DirectoryTrustPromptResponse::Answer {
                step: b"\r".to_vec()
            })
        );
    }
}
