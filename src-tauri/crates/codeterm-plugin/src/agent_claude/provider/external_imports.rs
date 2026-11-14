use codeterm_types::pty::PendingPrompt;
use std::sync::LazyLock;

pub(super) fn parse(screen: &str) -> Option<(PendingPrompt, usize)> {
    static FOOTER: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)^Enter\s+to\s+confirm\s*[·•]\s*Esc\s+to\s+cancel$").unwrap()
    });
    static OPTION: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)^([❯>])?\s*((?:No|Yes),\s*(?:disable|allow)\s+external\s+imports)$")
            .unwrap()
    });
    static TITLE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)^Allow\s+external\s+CLAUDE\.md\s+file\s+imports\?$").unwrap()
    });
    static PATH: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^(?:[A-Za-z]:[\\/]|/|~[\\/]|\\\\)").unwrap());
    let mut rows = screen
        .lines()
        .map(|row| {
            row.trim_matches(|c: char| {
                c.is_whitespace() || matches!(c, '│' | '┃' | '║' | '─' | '━' | '═')
            })
        })
        .collect::<Vec<_>>();
    while rows.last().is_some_and(|row| row.is_empty()) {
        rows.pop();
    }
    let footer = rows.len().checked_sub(1)?;
    if !FOOTER.is_match(rows[footer]) {
        return None;
    }
    let mut options = Vec::new();
    for line in (0..footer).rev() {
        if rows[line].is_empty() {
            continue;
        }
        let Some(captures) = OPTION.captures(rows[line]) else {
            break;
        };
        options.push((captures[2].to_string(), captures.get(1).is_some(), line));
    }
    options.reverse();
    if options.len() != 2
        || options.iter().filter(|option| option.1).count() != 1
        || !options[0].0.to_ascii_lowercase().starts_with("no, disable")
        || !options[1].0.to_ascii_lowercase().starts_with("yes, allow")
    {
        return None;
    }
    let before = &rows[..options[0].2];
    let title = before.iter().rposition(|row| TITLE.is_match(row))?;
    let context = &before[title + 1..];
    let imports = context
        .iter()
        .position(|row| row.eq_ignore_ascii_case("External imports:"))?;
    if !PATH.is_match(context.get(imports + 1)?)
        || context.iter().any(|row| row.starts_with(['❯', '>']))
    {
        return None;
    }
    let focused = options.iter().position(|option| option.1)?;
    Some((
        crate::prompt::build_prompt(
            "Claude is asking whether to allow external CLAUDE.md imports".into(),
            options
                .into_iter()
                .enumerate()
                .map(|(index, option)| (option.0, (index + 1).to_string()))
                .collect(),
        ),
        focused,
    ))
}
