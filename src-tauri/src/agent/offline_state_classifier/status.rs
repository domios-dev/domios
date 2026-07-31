use super::{active_turn_footer, parse_elapsed_ms};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub line: String,
    pub phase: Option<String>,
    pub elapsed_ms: Option<u64>,
}

const STATUS_SPINNERS: &[char] = &['✻', '✳', '✢', '✶', '✷', '✺', '✽', '●', '∗', '⋆'];
const STATUS_MESSAGE_MAX_CHARS: usize = 80;

fn compact_status_message(line: &str) -> String {
    let line = strip_status_chrome_tail(line).trim_end();
    let mut out = String::new();
    for (idx, ch) in line.chars().enumerate() {
        if idx >= STATUS_MESSAGE_MAX_CHARS {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn strip_status_chrome_tail(line: &str) -> &str {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut run_start = None;
    let mut run_len = 0usize;
    for (byte_idx, ch) in chars {
        if is_horizontal_chrome(ch) {
            if run_start.is_none() {
                run_start = Some(byte_idx);
            }
            run_len += 1;
            if run_len >= 3 {
                return &line[..run_start.unwrap_or(byte_idx)];
            }
        } else if ch.is_whitespace() || matches!(ch, '…' | '.') {
            continue;
        } else {
            run_start = None;
            run_len = 0;
        }
    }
    line
}

fn is_horizontal_chrome(ch: char) -> bool {
    matches!(ch, '\u{2500}'..='\u{257f}' | '—' | '―')
}

fn is_braille_spinner(c: char) -> bool {
    ('\u{2800}'..='\u{28ff}').contains(&c)
}

fn looks_like_path_only_status_text(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with('/')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
}

pub fn extract_status_line(screen_lines: &[String]) -> Option<StatusLine> {
    if let Some(status) = extract_opencode_status_line(screen_lines) {
        return Some(status);
    }
    if let Some(status) = extract_structural_status_line(screen_lines) {
        return Some(status);
    }
    let nonempty: Vec<&str> = screen_lines
        .iter()
        .map(|r| r.trim_end())
        .filter(|r| !r.is_empty())
        .collect();
    let start = nonempty.len().saturating_sub(16);
    for row in nonempty[start..].iter().rev() {
        let row = *row;
        let has_ellipsis = row.contains('…') || row.contains("...");
        let elapsed_ms = parse_elapsed_ms(row);
        let has_token = row.to_lowercase().contains("tok");
        let strong_spinner = row.chars().any(|c| STATUS_SPINNERS.contains(&c));
        // A just-started turn renders `✳ Verb…` with no elapsed/token suffix yet.
        let leading_spinner = row
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| STATUS_SPINNERS.contains(&c));
        let is_widget = (has_ellipsis && (elapsed_ms.is_some() || has_token))
            || (strong_spinner && elapsed_ms.is_some())
            || (leading_spinner && has_ellipsis);
        if !is_widget {
            continue;
        }
        let line = row.trim_start_matches(|c: char| {
            STATUS_SPINNERS.contains(&c) || c == '·' || c.is_whitespace()
        });
        return Some(StatusLine {
            line: compact_status_message(line),
            phase: Some("work".to_string()),
            elapsed_ms,
        });
    }
    None
}

fn extract_structural_status_line(screen_lines: &[String]) -> Option<StatusLine> {
    if !active_turn_footer(screen_lines) {
        return None;
    }
    let nonempty: Vec<&str> = screen_lines
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    let start = nonempty.len().saturating_sub(14);
    for row in nonempty[start..].iter().rev() {
        if let Some(status) = extract_braille_status_row(row) {
            return Some(status);
        }
        if let Some(status) = extract_elapsed_interrupt_status_row(row) {
            return Some(status);
        }
    }
    None
}

fn extract_braille_status_row(row: &str) -> Option<StatusLine> {
    let trimmed = row.trim_start();
    let mut chars = trimmed.char_indices().peekable();
    let mut end = 0usize;
    let mut count = 0usize;
    while let Some((idx, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            end = idx + ch.len_utf8();
            chars.next();
            continue;
        }
        if is_braille_spinner(ch) {
            end = idx + ch.len_utf8();
            count += 1;
            chars.next();
            continue;
        }
        break;
    }
    if count == 0 {
        return None;
    }
    let line = trimmed[end..].trim();
    if line.is_empty() || line.len() > 80 {
        return None;
    }
    if looks_like_path_only_status_text(line) {
        return None;
    }
    Some(StatusLine {
        line: compact_status_message(line),
        phase: Some("work".to_string()),
        elapsed_ms: None,
    })
}

fn extract_elapsed_interrupt_status_row(row: &str) -> Option<StatusLine> {
    let trimmed = row.trim();
    let lower = trimmed.to_lowercase();
    if !(lower.contains("esc to interrupt") || lower.contains("esc interrupt")) {
        return None;
    }
    let elapsed_ms = parse_elapsed_ms(trimmed)?;
    let line = trimmed
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '•' | '*' | '-' | '·' | '●'))
        .to_string();
    if line.is_empty() || line.len() > 100 {
        return None;
    }
    Some(StatusLine {
        line: compact_status_message(&line),
        phase: Some("work".to_string()),
        elapsed_ms: Some(elapsed_ms),
    })
}

fn extract_opencode_status_line(screen_lines: &[String]) -> Option<StatusLine> {
    if !active_turn_footer(screen_lines) {
        return None;
    }
    let nonempty: Vec<&str> = screen_lines
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    let start = nonempty.len().saturating_sub(14);
    let mut fallback = None;
    for row in nonempty[start..].iter().rev() {
        let row = *row;
        if is_opencode_chrome_row(row) {
            continue;
        }
        let Some((phase, trimmed)) = strip_opencode_status_prefix(row) else {
            continue;
        };
        if trimmed.is_empty() || trimmed.len() > 80 {
            continue;
        }
        if looks_like_path_only_status_text(trimmed) {
            continue;
        }
        let status = StatusLine {
            line: compact_status_message(trimmed),
            phase: Some(phase.clone()),
            elapsed_ms: None,
        };
        if phase == "work" {
            fallback.get_or_insert(status);
            continue;
        }
        return Some(status);
    }
    fallback
}

fn is_opencode_chrome_row(row: &str) -> bool {
    let trimmed = row.trim_start();
    let lower = trimmed.to_lowercase();
    if lower.contains("esc to interrupt") || lower.contains("esc interrupt") {
        return true;
    }
    trimmed.starts_with('┃')
        || trimmed.starts_with('╹')
        || trimmed.starts_with('│')
        || trimmed
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '─' | '━' | '╹' | '╺' | '╸'))
}

fn strip_opencode_status_prefix(row: &str) -> Option<(String, &str)> {
    let trimmed = row.trim_start();
    let mut chars = trimmed.char_indices();
    let (_, first) = chars.next()?;
    let phase = if is_braille_spinner(first) {
        "thinking"
    } else if first == '~' {
        "streaming"
    } else if first == '▣' {
        "work"
    } else {
        return None;
    };
    let rest_start = first.len_utf8();
    Some((phase.to_string(), trimmed[rest_start..].trim()))
}

pub fn has_active_turn_footer(screen_lines: &[String]) -> bool {
    active_turn_footer(screen_lines)
}

fn sum_hms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut total = 0u64;
    let mut found = false;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let mult = match b.get(i) {
                Some(b'h') => 3_600_000,
                Some(b'm') => 60_000,
                Some(b's') => 1_000,
                _ => 0,
            };
            if mult > 0 {
                if let Ok(n) = s[start..i].parse::<u64>() {
                    total += n * mult;
                    found = true;
                }
                i += 1;
                continue;
            }
        }
        i += 1;
    }
    if found {
        Some(total)
    } else {
        None
    }
}

pub fn extract_completion_summary(screen_lines: &[String]) -> Option<StatusLine> {
    let nonempty: Vec<&str> = screen_lines
        .iter()
        .map(|r| r.trim_end())
        .filter(|r| !r.is_empty())
        .collect();
    let start = nonempty.len().saturating_sub(16);
    for row in nonempty[start..].iter().rev() {
        let row = *row;
        let ellipsis = row.contains('…') || row.contains("...");
        let leading_spinner = row
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| STATUS_SPINNERS.contains(&c));
        // An active spinner below the summary means a NEW turn is live: the
        // stale summary above it must not read as the current state.
        if leading_spinner && ellipsis {
            return None;
        }
        if ellipsis || row.contains('(') {
            continue;
        }
        if !row.chars().any(|c| STATUS_SPINNERS.contains(&c)) {
            continue;
        }
        let Some(elapsed_ms) = sum_hms(row) else {
            continue;
        };
        let line = row.trim_start_matches(|c: char| {
            STATUS_SPINNERS.contains(&c) || c == '·' || c.is_whitespace()
        });
        return Some(StatusLine {
            line: compact_status_message(line),
            phase: Some("done".to_string()),
            elapsed_ms: Some(elapsed_ms),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_status_line;

    fn lines(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|row| (*row).to_string()).collect()
    }

    #[test]
    fn braille_path_footer_is_not_status_line() {
        let screen = lines(&[
            "Tool output",
            "⠂ /.codeterm-dev/general",
            "→ Working (ctrl+c to stop)",
        ]);

        assert_eq!(extract_status_line(&screen), None);
    }

    #[test]
    fn braille_action_row_is_status_line() {
        let screen = lines(&["Tool output", "⠞ Composing", "→ Working (ctrl+c to stop)"]);

        let status = extract_status_line(&screen).expect("status line");
        assert_eq!(status.line, "Composing");
    }

    // counter + a trailing "thinking with high effort" phase word.
    #[test]
    fn current_gen_thinking_footer_is_status_line() {
        let screen = lines(&[
            "⏺ Write(fib.py)",
            "✽ Inferring… (58s · ↓ 797 tokens · thinking with high effort)",
        ]);
        let status = extract_status_line(&screen).expect("status line");
        assert_eq!(status.phase.as_deref(), Some("work"));
        assert_eq!(status.elapsed_ms, Some(58_000));
        assert!(status.line.starts_with("Inferring…"), "{}", status.line);
    }

    // Novel/unseen verb must still parse as a working widget purely by structure.
    #[test]
    fn novel_verb_footer_is_status_line_by_structure() {
        let screen = lines(&["✽ Xyzzying… (2s · ↓ 5 tokens)"]);
        let status = extract_status_line(&screen).expect("status line");
        assert_eq!(status.elapsed_ms, Some(2_000));
    }

    #[test]
    fn status_line_drops_terminal_divider_tail() {
        let screen = lines(&[
            "Assistant text",
            "✻ Creating… (4s · ↓64 tokens)────────────────────────────────────────…",
        ]);

        let status = extract_status_line(&screen).expect("status line");

        assert_eq!(status.line, "Creating… (4s · ↓64 tokens)");
    }

    #[test]
    fn bare_just_started_spinner_is_a_status_line() {
        let screen = lines(&["✳ Fluttering…"]);
        let status = extract_status_line(&screen).expect("status line");
        assert_eq!(status.phase.as_deref(), Some("work"));
        assert_eq!(status.elapsed_ms, None);
        assert!(status.line.starts_with("Fluttering…"), "{}", status.line);
    }

    #[test]
    fn fresh_spinner_below_summary_suppresses_stale_completion() {
        let screen = lines(&[
            "✻ Baked for 2m 56s",
            "❯ Add an optional wall-wrap mode toggled by a checkbox on the start screen.",
            "✳ Fluttering…",
            "❯",
        ]);
        assert!(super::extract_completion_summary(&screen).is_none());
    }

    #[test]
    fn completion_summary_still_extracts_after_turn_ends() {
        let screen = lines(&["assistant prose", "✻ Baked for 2m 56s", "❯"]);
        let summary = super::extract_completion_summary(&screen).expect("summary");
        assert_eq!(summary.line, "Baked for 2m 56s");
    }
}
