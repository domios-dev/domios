use super::{bounded_text, CuratorRound, HISTORY_LIMIT, MAX_DIGEST_BYTES};

pub struct SynthesisGoalInput<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub vision: &'a str,
    pub vector: &'a str,
    pub open_criteria: usize,
}

pub fn context_prefix(input: SynthesisGoalInput<'_>, base: &str) -> String {
    let goal = format!(
        "\ngoal [{}]: {} | {}\ncorrectness vector [{}]: {}\nopen acceptance criteria [{}]: {}\n",
        input.id,
        bounded_text(input.title, 256),
        bounded_text(input.vision, 512),
        input.id,
        bounded_text(input.vector, 512),
        input.id,
        input.open_criteria
    );
    let mut body =
        bounded_text(base, (MAX_DIGEST_BYTES - 2048).saturating_sub(goal.len())).to_string();
    body.push_str(&goal);
    body
}

pub fn history_budget(prefix: &str) -> usize {
    HISTORY_LIMIT.min((MAX_DIGEST_BYTES - 2048).saturating_sub(prefix.len()))
}

pub fn history_learning_kind(text: &str) -> &'static str {
    if text.starts_with(['+', '-']) {
        "RETRO"
    } else {
        "LEARN"
    }
}

pub fn round_history_lines(
    round: CuratorRound,
    fallback: impl IntoIterator<Item = String>,
) -> Vec<(&'static str, String)> {
    let mut lines = vec![(
        "MOVE",
        if round.move_line.is_empty() {
            round.summary
        } else {
            round.move_line
        },
    )];
    let mut retro = round.retro;
    let mut learn = round.learn;
    if retro.is_empty() && learn.is_empty() {
        for text in fallback {
            if history_learning_kind(&text) == "RETRO" {
                retro.push(text);
            } else {
                learn.push(text);
            }
        }
    }
    lines.extend(retro.into_iter().map(|line| ("RETRO", line)));
    lines.extend(learn.into_iter().map(|line| ("LEARN", line)));
    lines
}

pub fn history_line(id: &str, kind: &str, text: &str) -> String {
    format!("{id} | {kind} | {}\n", text.replace(['\n', '\r'], " "))
}
