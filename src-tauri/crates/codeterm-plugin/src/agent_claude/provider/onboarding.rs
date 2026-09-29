use crate::provider::ProviderPromptKeyStep;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Prompt {
    pub question: String,
    pub options: Vec<(String, String)>,
    pub frame_key: String,
    pub step: Vec<u8>,
}

#[derive(Debug)]
struct OptionRow {
    focused: bool,
    number: String,
    label: String,
    line: usize,
}

pub(super) fn parse(screen: &str) -> Option<Prompt> {
    let clean_screen = strip_csi(screen);
    let lines = clean_screen.lines().map(clean_row).collect::<Vec<_>>();
    let candidates = lines
        .iter()
        .enumerate()
        .filter_map(|(line, row)| parse_option(row, line))
        .collect::<Vec<_>>();
    let options = candidates.get(candidates.len().checked_sub(3)?..)?;
    if options
        .iter()
        .enumerate()
        .any(|(index, option)| option.number != (index + 1).to_string())
        || options.iter().filter(|option| option.focused).count() != 1
    {
        return None;
    }
    let first = options.first()?.line;
    let last = options.last()?.line;
    if last.saturating_sub(first) > 4 {
        return None;
    }
    let footer = lines
        .get(last + 1..(last + 4).min(lines.len()))?
        .join(" ")
        .to_ascii_lowercase();
    if !footer.contains("enter") || !footer.contains("esc") {
        return None;
    }
    let question = (first.saturating_sub(5)..first)
        .rev()
        .map(|index| lines[index].trim())
        .find(|row| row.ends_with('?'))?
        .to_string();
    let question_words = words(&question);
    if !question_words.iter().any(|word| word == "auto")
        || !question_words.iter().any(|word| word == "environment")
    {
        return None;
    }
    let labels = options
        .iter()
        .map(|option| normalize_label(&option.label))
        .collect::<Vec<_>>();
    let target = labels
        .iter()
        .position(|label| label.starts_with("not now"))?;
    if labels
        .iter()
        .filter(|label| label.starts_with("yes"))
        .count()
        != 1
        || labels
            .iter()
            .filter(|label| label.starts_with("don't show again"))
            .count()
            != 1
    {
        return None;
    }
    let current = options.iter().position(|option| option.focused)?;
    let step = if current == target {
        b"\r".to_vec()
    } else if target > current {
        b"j".to_vec()
    } else {
        b"k".to_vec()
    };
    Some(Prompt {
        question,
        options: options
            .iter()
            .map(|option| (option.label.clone(), option.number.clone()))
            .collect(),
        frame_key: format!("onboarding:focus-{current}:target-{target}"),
        step,
    })
}

pub(super) fn response(screen: &str) -> Option<ProviderPromptKeyStep> {
    let prompt = parse(screen)?;
    Some(ProviderPromptKeyStep {
        frame_key: prompt.frame_key,
        step: prompt.step,
    })
}

fn strip_csi(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' && characters.peek() == Some(&'[') {
            characters.next();
            for control in characters.by_ref() {
                if ('@'..='~').contains(&control) {
                    break;
                }
            }
        } else {
            output.push(character);
        }
    }
    output
}

fn parse_option(row: &str, line: usize) -> Option<OptionRow> {
    let focused = row.starts_with('❯') || row.starts_with('>');
    let content = row
        .trim_start_matches(|character| character == '❯' || character == '>')
        .trim();
    let (number, label) = content.split_once(". ")?;
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    Some(OptionRow {
        focused,
        number: number.to_string(),
        label: label.trim().to_string(),
        line,
    })
}

fn clean_row(row: &str) -> String {
    row.trim()
        .trim_start_matches(|character| matches!(character, '│' | '┃' | '║'))
        .trim()
        .trim_end_matches(|character| matches!(character, '│' | '┃' | '║'))
        .trim()
        .to_string()
}

fn words(value: &str) -> Vec<String> {
    value
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_label(value: &str) -> String {
    value
        .to_lowercase()
        .replace('’', "'")
        .replace('‘', "'")
        .trim()
        .to_string()
}
