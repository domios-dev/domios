use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AttributedColor {
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributedCell {
    pub ch: String,
    pub fg: AttributedColor,
    pub bg: AttributedColor,
    pub inverse: bool,
    pub bold: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributedScreen {
    pub rows: u16,
    pub cols: u16,
    pub cells: Vec<Vec<AttributedCell>>,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub dominant_bg: AttributedColor,
    pub dominant_fg: AttributedColor,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickerDetection {
    pub question: String,
    pub options: Vec<String>,
    pub selected_index: usize,
    pub multi_select: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarkerClass {
    Arrow,
    Checkbox,
    Bullet,
    Numbered,
    Lettered,
    Plus,
    Plain,
}

#[derive(Clone, Debug)]
struct RowFeature {
    row: u16,
    text: String,
    clean: String,
    indent: usize,
    class: MarkerClass,
    selected_score: usize,
    footer: bool,
    border: bool,
    option_like: bool,
    multi_select: bool,
}

impl AttributedScreen {
    pub fn from_vt100(screen: &vt100::Screen) -> Self {
        let (rows, cols) = screen.size();
        let mut cells = Vec::with_capacity(rows as usize);
        let mut bg_counts = BTreeMap::<AttributedColor, usize>::new();
        let mut fg_counts = BTreeMap::<AttributedColor, usize>::new();

        for row in 0..rows {
            let mut out_row = Vec::with_capacity(cols as usize);
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    out_row.push(default_cell());
                    continue;
                };
                let ch = cell.contents().to_string();
                let fg = convert_color(cell.fgcolor());
                let bg = convert_color(cell.bgcolor());
                if cell.has_contents() {
                    *fg_counts.entry(fg.clone()).or_default() += 1;
                    *bg_counts.entry(bg.clone()).or_default() += 1;
                }
                out_row.push(AttributedCell {
                    ch,
                    fg,
                    bg,
                    inverse: cell.inverse(),
                    bold: cell.bold(),
                });
            }
            cells.push(out_row);
        }

        let dominant_bg = dominant(bg_counts);
        let dominant_fg = dominant(fg_counts);
        let (cursor_row, cursor_col) = screen.cursor_position();

        Self {
            rows,
            cols,
            cells,
            cursor_row,
            cursor_col,
            dominant_bg,
            dominant_fg,
        }
    }

    pub fn row_text(&self, row: u16) -> String {
        self.cells
            .get(row as usize)
            .map(|cells| cells.iter().map(|c| cell_char(c)).collect::<String>())
            .unwrap_or_default()
    }
}

pub fn detect_picker(screen: &AttributedScreen) -> Option<PickerDetection> {
    let rows: Vec<RowFeature> = (0..screen.rows)
        .map(|row| analyze_row(screen, row))
        .collect();
    let mut blocks = Vec::new();
    let mut start: Option<usize> = None;

    for (idx, row) in rows.iter().enumerate() {
        if row.option_like {
            if start.is_none() {
                start = Some(idx);
            }
            continue;
        }
        if let Some(top) = start.take() {
            push_block(&rows, top, idx, &mut blocks);
        }
    }
    if let Some(top) = start {
        push_block(&rows, top, rows.len(), &mut blocks);
    }
    push_selected_expansions(&rows, &mut blocks);

    blocks
        .into_iter()
        .filter_map(|(top, bottom)| score_block(screen, &rows, top, bottom))
        .max_by_key(|candidate| candidate.0)
        .map(|(_, detection)| detection)
}

fn convert_color(color: vt100::Color) -> AttributedColor {
    match color {
        vt100::Color::Default => AttributedColor::Default,
        vt100::Color::Idx(i) => AttributedColor::Idx(i),
        vt100::Color::Rgb(r, g, b) => AttributedColor::Rgb(r, g, b),
    }
}

fn dominant(counts: BTreeMap<AttributedColor, usize>) -> AttributedColor {
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(color, _)| color)
        .unwrap_or(AttributedColor::Default)
}

fn default_cell() -> AttributedCell {
    AttributedCell {
        ch: String::new(),
        fg: AttributedColor::Default,
        bg: AttributedColor::Default,
        inverse: false,
        bold: false,
    }
}

fn cell_char(cell: &AttributedCell) -> &str {
    if cell.ch.is_empty() {
        " "
    } else {
        &cell.ch
    }
}

fn analyze_row(screen: &AttributedScreen, row: u16) -> RowFeature {
    let text = screen.row_text(row);
    let trimmed_end = text.trim_end().to_string();
    let stripped = strip_frame(&trimmed_end);
    let clean = clean_option_label(&stripped);
    let indent = stripped.chars().take_while(|c| c.is_whitespace()).count();
    let class = marker_class(stripped.trim_start());
    let footer = is_footer_hint(&stripped);
    let border = is_box_border(&stripped);
    let multi_select = contains_checkbox(&stripped) || contains_select_all_hint(&stripped);
    let selected_score = selected_score(screen, row, &stripped, class);
    let option_like = !clean.is_empty()
        && !footer
        && !border
        && (class != MarkerClass::Plain || selected_score >= 3 || contains_checkbox(&stripped));

    RowFeature {
        row,
        text: stripped,
        clean,
        indent,
        class,
        selected_score,
        footer,
        border,
        option_like,
        multi_select,
    }
}

fn push_block(rows: &[RowFeature], top: usize, bottom: usize, blocks: &mut Vec<(usize, usize)>) {
    if bottom <= top {
        return;
    }
    let mut segment_top = top;
    for idx in top + 1..bottom {
        let prev = &rows[idx - 1];
        let row = &rows[idx];
        if !parallel_rows(prev, row) {
            if idx > segment_top {
                blocks.push((segment_top, idx));
            }
            segment_top = idx;
        }
    }
    if bottom > segment_top {
        blocks.push((segment_top, bottom));
    }
}

fn push_selected_expansions(rows: &[RowFeature], blocks: &mut Vec<(usize, usize)>) {
    for (idx, row) in rows.iter().enumerate() {
        if row.selected_score == 0 || row.clean.is_empty() || row.footer || row.border {
            continue;
        }
        let mut top = idx;
        while top > 0 && same_plain_option_group(&rows[top - 1], row) {
            top -= 1;
        }
        let mut bottom = idx + 1;
        while bottom < rows.len() && same_plain_option_group(&rows[bottom], row) {
            bottom += 1;
        }
        if bottom > top {
            blocks.push((top, bottom));
        }
    }
}

fn same_plain_option_group(candidate: &RowFeature, selected: &RowFeature) -> bool {
    !candidate.clean.is_empty()
        && !candidate.footer
        && !candidate.border
        && candidate.indent == selected.indent
        && candidate.class == selected.class
}

fn parallel_rows(a: &RowFeature, b: &RowFeature) -> bool {
    a.class == b.class || a.indent == b.indent || a.selected_score > 0 || b.selected_score > 0
}

fn score_block(
    screen: &AttributedScreen,
    rows: &[RowFeature],
    top: usize,
    bottom: usize,
) -> Option<(usize, PickerDetection)> {
    let block = &rows[top..bottom];
    if block.is_empty() {
        return None;
    }

    let footer_near = rows[bottom..rows.len().min(bottom + 5)]
        .iter()
        .any(|row| row.footer);
    let selected = block.iter().enumerate().fold((0, 0), |best, (idx, row)| {
        if row.selected_score > best.1 {
            (idx, row.selected_score)
        } else {
            best
        }
    });
    let cursor_near = block
        .iter()
        .any(|row| row.row.abs_diff(screen.cursor_row) <= 1);

    if block.len() == 1 && selected.1 < 5 && !footer_near {
        return None;
    }
    if block.len() < 3 && !footer_near {
        return None;
    }
    if selected.1 == 0 && !footer_near {
        return None;
    }
    if looks_like_banner(rows, top, bottom, footer_near, cursor_near) {
        return None;
    }

    let question = find_question(rows, top)?;
    let options = block
        .iter()
        .map(|row| row.clean.clone())
        .collect::<Vec<_>>();
    let multi_select = block.iter().any(|row| row.multi_select)
        || rows[bottom..rows.len().min(bottom + 5)]
            .iter()
            .any(|row| contains_select_all_hint(&row.text));
    let score = selected.1 * 10 + block.len() + usize::from(footer_near) * 5;

    Some((
        score,
        PickerDetection {
            question,
            options,
            selected_index: selected.0,
            multi_select,
        },
    ))
}

fn selected_score(screen: &AttributedScreen, row: u16, text: &str, class: MarkerClass) -> usize {
    let mut score = 0;
    let leading = text.trim_start();
    let starts_in_side_band = leading
        .chars()
        .next()
        .is_some_and(|ch| is_vertical_border(ch) || ch == '╹');
    if has_selected_glyph(leading) {
        score += 5;
    }
    if matches!(class, MarkerClass::Checkbox) && has_checked_box(leading) {
        score += 4;
    }

    let mut inverse = 0usize;
    let mut bg_delta = 0usize;
    let mut fg_delta = 0usize;
    let row_cells = screen.cells.get(row as usize).into_iter().flatten();
    let mut bg_counts = BTreeMap::<AttributedColor, usize>::new();
    for cell in row_cells.clone() {
        if !cell.ch.trim().is_empty() {
            *bg_counts.entry(cell.bg.clone()).or_default() += 1;
        }
    }
    let row_bg = dominant(bg_counts);
    let mut local_highlight = 0usize;
    for cell in row_cells {
        if cell.ch.trim().is_empty() {
            continue;
        }
        if cell.inverse {
            inverse += 1;
        }
        if cell.bg != screen.dominant_bg {
            bg_delta += 1;
        }
        if cell.bg != screen.dominant_bg
            && cell.bg != row_bg
            && !is_border_char(cell_char(cell).chars().next().unwrap_or(' '))
            && !starts_in_side_band
        {
            local_highlight += 1;
        }
        if cell.fg != screen.dominant_fg && cell.fg != AttributedColor::Default {
            fg_delta += 1;
        }
    }
    if inverse > 0 {
        score += 4;
    }
    if bg_delta >= 2 {
        score += 3;
    }
    if local_highlight >= 2 {
        score += 5;
    }
    if fg_delta >= 1 && has_selected_glyph(leading) {
        score += 2;
    }
    if row == screen.cursor_row {
        score += 1;
    }
    score
}

fn find_question(rows: &[RowFeature], top: usize) -> Option<String> {
    rows[..top]
        .iter()
        .rev()
        .find(|row| is_question_candidate(row))
        .map(|row| clean_question(&row.text))
        .filter(|question| !question.is_empty())
}

fn is_question_candidate(row: &RowFeature) -> bool {
    if row.clean.is_empty() || row.footer || row.border {
        return false;
    }
    let lower = row.clean.to_lowercase();
    lower != "search" && !lower.starts_with("view all ")
}

fn looks_like_banner(
    rows: &[RowFeature],
    top: usize,
    bottom: usize,
    footer_near: bool,
    cursor_near: bool,
) -> bool {
    if footer_near || cursor_near {
        return false;
    }
    let border_above = rows[..top].iter().rev().take(2).any(|row| row.border);
    let border_below = rows[bottom..].iter().take(2).any(|row| row.border);
    border_above && border_below
}

fn strip_frame(text: &str) -> String {
    let trimmed = text.trim_end();
    let chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() >= 2
        && is_vertical_border(chars[0])
        && is_vertical_border(*chars.last().unwrap())
    {
        chars[1..chars.len() - 1].iter().collect::<String>()
    } else {
        trimmed.to_string()
    }
}

fn clean_question(text: &str) -> String {
    let question = strip_marker_prefix(text)
        .trim()
        .trim_matches(|c| is_vertical_border(c) || c == '─' || c == '━')
        .trim()
        .to_string();
    question
        .strip_suffix("esc")
        .unwrap_or(&question)
        .trim_end()
        .to_string()
}

fn clean_option_label(text: &str) -> String {
    strip_marker_prefix(text).trim().to_string()
}

fn strip_marker_prefix(text: &str) -> String {
    let mut s = text.trim_start().to_string();
    for glyph in ["❯", "›", ">", "→", "➜", "▸", "▶", "●", "•", "-", "*"] {
        if let Some(rest) = s.strip_prefix(glyph) {
            s = rest.trim_start().to_string();
            break;
        }
    }
    for prefix in ["[ ]", "[x]", "[X]", "( )", "(x)", "(X)", "☐", "☑", "☒"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim_start().to_string();
            break;
        }
    }
    if let Some((prefix, rest)) = s.split_once(['.', ')']) {
        let token = prefix.trim();
        if !token.is_empty() && token.len() <= 3 && token.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return rest.trim_start().to_string();
        }
    }
    s
}

fn marker_class(text: &str) -> MarkerClass {
    if contains_checkbox(text) {
        return MarkerClass::Checkbox;
    }
    if has_selected_glyph(text) || starts_with_any(text, &[">", "›", "→", "➜", "▸", "▶", "●"])
    {
        return MarkerClass::Arrow;
    }
    if starts_with_any(text, &["●", "•", "-", "*"]) {
        return MarkerClass::Bullet;
    }
    if text.starts_with("+ ") {
        return MarkerClass::Plus;
    }
    if marker_token(text).is_some_and(|token| token.chars().all(|c| c.is_ascii_digit())) {
        return MarkerClass::Numbered;
    }
    if marker_token(text).is_some_and(|token| token.chars().all(|c| c.is_ascii_alphabetic())) {
        return MarkerClass::Lettered;
    }
    MarkerClass::Plain
}

fn marker_token(text: &str) -> Option<&str> {
    let (prefix, _) = text.split_once(['.', ')'])?;
    let token = prefix.trim();
    if !token.is_empty() && token.len() <= 3 {
        Some(token)
    } else {
        None
    }
}

fn has_selected_glyph(text: &str) -> bool {
    starts_with_any(text, &["❯", "›", ">", "→", "➜", "▸", "▶", "●"])
}

fn contains_checkbox(text: &str) -> bool {
    ["[ ]", "[x]", "[X]", "( )", "(x)", "(X)", "☐", "☑", "☒"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn has_checked_box(text: &str) -> bool {
    ["[x]", "[X]", "(x)", "(X)", "☑", "☒"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn contains_select_all_hint(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("select all") || lower.contains("space select") || lower.contains("tab select")
}

fn is_footer_hint(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("enter to select")
        || lower.contains("space select")
        || lower.contains("tab toggle")
        || lower.contains("tab select")
        || lower.contains("esc dismiss")
        || lower.contains("esc to")
        || lower.contains("←")
        || lower.contains("→")
}

fn is_box_border(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && (trimmed.chars().filter(|c| is_border_char(*c)).count() * 2 >= trimmed.chars().count()
            || trimmed.chars().filter(|c| is_border_char(*c)).count() >= 6)
}

fn is_border_char(c: char) -> bool {
    matches!(
        c,
        '─' | '━'
            | '│'
            | '┃'
            | '┌'
            | '┐'
            | '└'
            | '┘'
            | '├'
            | '┤'
            | '┬'
            | '┴'
            | '┼'
            | '╭'
            | '╮'
            | '╰'
            | '╯'
            | '═'
            | '║'
            | '╔'
            | '╗'
            | '╚'
            | '╝'
            | '▀'
            | '╹'
    )
}

fn is_vertical_border(c: char) -> bool {
    matches!(c, '│' | '┃' | '║')
}

fn starts_with_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.starts_with(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bg_selected_picker_with_footer() {
        let screen = screen_from_rows(&[
            ("Choose a model", false),
            ("  Auto", false),
            ("  Composer", true),
            ("  Opus", false),
            ("Enter to select", false),
        ]);

        let detected = detect_picker(&screen).expect("picker");

        assert_eq!(detected.question, "Choose a model");
        assert_eq!(detected.options, ["Auto", "Composer", "Opus"]);
        assert_eq!(detected.selected_index, 1);
        assert!(!detected.multi_select);
    }

    #[test]
    fn rejects_boxed_banner_without_footer() {
        let screen = screen_from_rows(&[
            ("┌────────────┐", false),
            ("│ What's new │", true),
            ("│ Read docs  │", true),
            ("└────────────┘", false),
        ]);

        assert_eq!(detect_picker(&screen), None);
    }

    fn screen_from_rows(rows: &[(&str, bool)]) -> AttributedScreen {
        let cols = rows
            .iter()
            .map(|(text, _)| text.chars().count())
            .max()
            .unwrap_or(0) as u16;
        let cells = rows
            .iter()
            .map(|(text, selected)| {
                let mut row = text
                    .chars()
                    .map(|ch| AttributedCell {
                        ch: ch.to_string(),
                        fg: AttributedColor::Default,
                        bg: if *selected {
                            AttributedColor::Rgb(30, 30, 30)
                        } else {
                            AttributedColor::Default
                        },
                        inverse: false,
                        bold: false,
                    })
                    .collect::<Vec<_>>();
                row.resize_with(cols as usize, default_cell);
                row
            })
            .collect::<Vec<_>>();

        AttributedScreen {
            rows: rows.len() as u16,
            cols,
            cells,
            cursor_row: 2,
            cursor_col: 0,
            dominant_bg: AttributedColor::Default,
            dominant_fg: AttributedColor::Default,
        }
    }
}
