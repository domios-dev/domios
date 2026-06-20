use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod horizontal_buttons;
mod picker_helpers;

use horizontal_buttons::detect_horizontal_button_row;
use picker_helpers::{
    is_border_char, looks_like_path, looks_like_section_tabs, row_is_bold, shortcut_token,
    shortcut_token_and_rest, starts_with_any, strip_side_band, strip_side_preview,
};

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
    KeyShortcut,
    Bullet,
    Numbered,
    Lettered,
    Plus,
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OptionKind {
    Checkbox,
    KeyShortcut,
    Numbered,
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
    side_band: bool,
    content_bg: AttributedColor,
    bold: bool,
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

    if let Some(detection) = detect_horizontal_button_row(screen, &rows) {
        return Some(detection);
    }
    if let Some(detection) = detect_title_search_panel(&rows) {
        return Some(detection);
    }
    if let Some(detection) = detect_footer_panel(&rows) {
        return Some(detection);
    }
    if let Some(detection) = detect_selected_row_block(screen, &rows) {
        return Some(detection);
    }

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

fn row_content_bg(screen: &AttributedScreen, row: u16) -> AttributedColor {
    let mut counts = BTreeMap::<AttributedColor, usize>::new();
    for cell in screen.cells.get(row as usize).into_iter().flatten() {
        if !cell.ch.trim().is_empty() {
            *counts.entry(cell.bg.clone()).or_default() += 1;
        }
    }
    dominant(counts)
}

fn analyze_row(screen: &AttributedScreen, row: u16) -> RowFeature {
    let text = screen.row_text(row);
    let trimmed_end = text.trim_end().to_string();
    let framed = strip_frame(&trimmed_end);
    let side_band = framed.trim_start().starts_with('┃') || framed.trim_start().starts_with('╹');
    let stripped = strip_side_preview(&strip_side_band(&framed));
    let clean = clean_option_label(&stripped);
    let indent = stripped.chars().take_while(|c| c.is_whitespace()).count();
    let class = marker_class(stripped.trim_start());
    let footer = is_footer_hint(&stripped);
    let border = is_box_border(&stripped);
    let multi_select = contains_checkbox(&stripped) || contains_select_all_hint(&stripped);
    let selected_score = selected_score(screen, row, &framed, class);
    let content_bg = row_content_bg(screen, row);
    let bold = row_is_bold(screen, row);
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
        side_band,
        content_bg,
        bold,
    }
}

fn detect_title_search_panel(rows: &[RowFeature]) -> Option<PickerDetection> {
    let title_idx = rows.iter().position(|row| {
        let lower = row.text.to_lowercase();
        !row.clean.is_empty() && lower.ends_with("esc") && !lower.contains(" to ")
    })?;
    let search_idx = rows[title_idx + 1..]
        .iter()
        .position(|row| row.clean.eq_ignore_ascii_case("search"))
        .map(|idx| idx + title_idx + 1)?;

    let panel_bg = rows
        .get(search_idx)
        .map(|row| row.content_bg.clone())
        .unwrap_or(AttributedColor::Default);
    let mut options = Vec::new();
    let mut selected_index = None;
    for row in &rows[search_idx + 1..] {
        if row.clean.is_empty() {
            if !options.is_empty() {
                break;
            }
            continue;
        }
        if row.clean.to_lowercase().starts_with("view all ") {
            break;
        }
        let Some(label) = panel_option_label(row) else {
            continue;
        };
        if (is_panel_selection(row) || (!row.side_band && row.content_bg != panel_bg))
            && selected_index.is_none()
        {
            selected_index = Some(options.len());
        }
        options.push(label);
    }

    if options.len() < 2 {
        return None;
    }

    Some(PickerDetection {
        question: clean_question(&rows[title_idx].text),
        options,
        selected_index: selected_index.unwrap_or(0),
        multi_select: false,
    })
}

fn detect_footer_panel(rows: &[RowFeature]) -> Option<PickerDetection> {
    let footer_idx = rows.iter().position(|row| row.footer)?;
    let selected_idx = rows[..footer_idx]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, row)| is_strong_selection(row) && option_kind(row).is_some())
        .map(|(idx, _)| idx)?;
    let selected_kind = option_kind(&rows[selected_idx])?;

    match selected_kind {
        OptionKind::Checkbox | OptionKind::KeyShortcut | OptionKind::Numbered => {
            structured_footer_panel(rows, selected_idx, footer_idx, selected_kind)
        }
        OptionKind::Plain => plain_footer_panel(rows, selected_idx, footer_idx),
    }
}

fn structured_footer_panel(
    rows: &[RowFeature],
    selected_idx: usize,
    footer_idx: usize,
    selected_kind: OptionKind,
) -> Option<PickerDetection> {
    let mut top = selected_idx;
    while top > 0 && !rows[top - 1].border && !rows[top - 1].footer {
        top -= 1;
        if rows[top].clean.is_empty() {
            break;
        }
    }

    let mut options = Vec::new();
    let mut selected_index = None;
    for (idx, row) in rows[top..footer_idx].iter().enumerate() {
        let absolute = top + idx;
        if option_kind(row) != Some(selected_kind) {
            continue;
        }
        let Some(label) = group_option_label(row, selected_kind) else {
            continue;
        };
        if absolute == selected_idx {
            selected_index = Some(options.len());
        }
        options.push(label);
    }
    if options.len() < 2 {
        return None;
    }

    Some(PickerDetection {
        question: find_question(rows, top)?,
        options,
        selected_index: selected_index.unwrap_or(0),
        multi_select: selected_kind == OptionKind::Checkbox
            || rows[footer_idx].multi_select
            || contains_select_all_hint(&rows[footer_idx].text),
    })
}

fn plain_footer_panel(
    rows: &[RowFeature],
    selected_idx: usize,
    _footer_idx: usize,
) -> Option<PickerDetection> {
    let mut top = selected_idx;
    while top > 0 && group_option_label(&rows[top - 1], OptionKind::Plain).is_some() {
        top -= 1;
    }
    let mut bottom = selected_idx + 1;
    while bottom < rows.len() && group_option_label(&rows[bottom], OptionKind::Plain).is_some() {
        bottom += 1;
    }

    let mut options = Vec::new();
    let mut selected_index = None;
    for (idx, row) in rows[top..bottom].iter().enumerate() {
        let Some(label) = group_option_label(row, OptionKind::Plain) else {
            continue;
        };
        if top + idx == selected_idx {
            selected_index = Some(options.len());
        }
        options.push(label);
    }
    if options.len() < 2 {
        return None;
    }

    Some(PickerDetection {
        question: find_question(rows, top)?,
        options,
        selected_index: selected_index.unwrap_or(0),
        multi_select: false,
    })
}

fn detect_selected_row_block(
    screen: &AttributedScreen,
    rows: &[RowFeature],
) -> Option<PickerDetection> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| row.selected_score > 0)
        .filter_map(|(idx, row)| selected_row_candidate(screen, rows, idx, row))
        .max_by_key(|candidate| candidate.0)
        .map(|(_, detection)| detection)
}

fn selected_row_candidate(
    screen: &AttributedScreen,
    rows: &[RowFeature],
    selected_row_idx: usize,
    selected_row: &RowFeature,
) -> Option<(usize, PickerDetection)> {
    let selected_kind = option_kind(selected_row)?;
    let mut top = selected_row_idx;
    while top > 0 && group_option_label(&rows[top - 1], selected_kind).is_some() {
        top -= 1;
    }
    let mut bottom = selected_row_idx + 1;
    while bottom < rows.len() && group_option_label(&rows[bottom], selected_kind).is_some() {
        bottom += 1;
    }

    let mut options = Vec::new();
    let mut selected_index = None;
    for (idx, row) in rows[top..bottom].iter().enumerate() {
        let Some(label) = group_option_label(row, selected_kind) else {
            continue;
        };
        if top + idx == selected_row_idx {
            selected_index = Some(options.len());
        }
        options.push(label);
    }

    if options.len() < 2 {
        return None;
    }
    let footer_near = rows[bottom..rows.len().min(bottom + 5)]
        .iter()
        .any(|row| row.footer);
    if selected_kind == OptionKind::Plain && !footer_near {
        return None;
    }
    if !footer_near
        && !rows[top..bottom]
            .iter()
            .any(|row| row.row.abs_diff(screen.cursor_row) <= 1)
    {
        return None;
    }

    let question = find_question(rows, top)?;
    let multi_select = selected_kind == OptionKind::Checkbox
        || rows[bottom..rows.len().min(bottom + 5)]
            .iter()
            .any(|row| contains_select_all_hint(&row.text));
    let score = selected_row.selected_score * 10 + options.len() + usize::from(footer_near) * 5;

    Some((
        score,
        PickerDetection {
            question,
            options,
            selected_index: selected_index.unwrap_or(0),
            multi_select,
        },
    ))
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
    if block
        .iter()
        .all(|row| option_kind(row) == Some(OptionKind::Plain))
        && !footer_near
    {
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
    let mut marked: Option<&RowFeature> = None;
    let mut bold: Option<&RowFeature> = None;
    let mut nearest: Option<&RowFeature> = None;
    for (idx, row) in rows[..top].iter().enumerate().rev() {
        if !is_question_candidate(row) || heads_bullet_list(rows, idx, top) {
            continue;
        }
        if row.class == MarkerClass::Numbered || row.class == MarkerClass::Lettered {
            marked.get_or_insert(row);
        }
        if row.bold {
            bold.get_or_insert(row);
        }
        nearest.get_or_insert(row);
    }
    marked
        .or(bold)
        .or(nearest)
        .map(|row| clean_question(&row.text))
        .filter(|question| !question.is_empty())
}

fn heads_bullet_list(rows: &[RowFeature], idx: usize, top: usize) -> bool {
    rows[idx].class == MarkerClass::Plain
        && rows[idx + 1..top]
            .iter()
            .find(|row| !row.clean.is_empty())
            .is_some_and(|row| row.class == MarkerClass::Bullet)
}

fn is_question_candidate(row: &RowFeature) -> bool {
    if row.clean.is_empty() || row.footer || row.border {
        return false;
    }
    let lower = row.clean.to_lowercase();
    lower != "search"
        && lower != "filter:"
        && !lower.starts_with("view all ")
        && !looks_like_path(&row.clean)
        && !looks_like_section_tabs(&row.clean)
}

fn is_strong_selection(row: &RowFeature) -> bool {
    row.selected_score >= 3
        || has_selected_glyph(row.text.trim_start())
        || has_checked_box(&row.text)
}

fn is_panel_selection(row: &RowFeature) -> bool {
    row.selected_score >= 8
        || has_selected_glyph(row.text.trim_start())
        || has_checked_box(&row.text)
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
    let leading = chars.iter().take_while(|c| c.is_whitespace()).count();
    let trailing = chars.iter().rev().take_while(|c| c.is_whitespace()).count();
    let start = leading;
    let end = chars.len().saturating_sub(trailing);
    if end > start + 1 && is_vertical_border(chars[start]) && is_vertical_border(chars[end - 1]) {
        let inner = chars[start + 1..end - 1].iter().collect::<String>();
        format!("{}{}", " ".repeat(leading), inner)
    } else {
        trimmed.to_string()
    }
}

fn clean_question(text: &str) -> String {
    let stripped = text.trim_start();
    let stripped = stripped.strip_prefix('←').unwrap_or(stripped);
    let question = strip_marker_prefix(stripped)
        .trim()
        .trim_matches(|c| is_vertical_border(c) || c == '─' || c == '━')
        .trim()
        .to_string();
    let question = question
        .split("Max mode:")
        .next()
        .unwrap_or(&question)
        .trim_end();
    let question = question
        .strip_suffix("esc")
        .unwrap_or(&question)
        .trim_end()
        .to_string();
    question
        .split(" — ")
        .last()
        .unwrap_or(&question)
        .split(" (Select all that apply)")
        .next()
        .unwrap_or(&question)
        .trim()
        .to_string()
}

fn clean_option_label(text: &str) -> String {
    display_label(&strip_marker_prefix(text))
}

fn panel_option_label(row: &RowFeature) -> Option<String> {
    let lower = row.clean.to_lowercase();
    if row.clean.is_empty()
        || matches!(
            lower.as_str(),
            "search" | "popular" | "popular providers" | "providers" | "ask" | "buil" | "build"
        )
    {
        return None;
    }
    group_option_label(row, option_kind(row).unwrap_or(OptionKind::Plain))
}

fn group_option_label(row: &RowFeature, selected_kind: OptionKind) -> Option<String> {
    if row.clean.is_empty() || row.footer || row.border {
        return None;
    }
    let kind = option_kind(row)?;
    let compatible = match selected_kind {
        OptionKind::Checkbox => kind == OptionKind::Checkbox,
        OptionKind::KeyShortcut => kind == OptionKind::KeyShortcut,
        OptionKind::Numbered => kind == OptionKind::Numbered,
        OptionKind::Plain => kind == OptionKind::Plain,
    };
    compatible.then(|| row.clean.clone())
}

fn option_kind(row: &RowFeature) -> Option<OptionKind> {
    let text = strip_selected_glyph(row.text.trim_start());
    if contains_checkbox(text) {
        return Some(OptionKind::Checkbox);
    }
    if shortcut_token(text).is_some() {
        return Some(OptionKind::KeyShortcut);
    }
    if marker_token(text).is_some_and(|token| token.chars().all(|c| c.is_ascii_digit())) {
        return Some(OptionKind::Numbered);
    }
    is_plain_option_label(&row.clean).then_some(OptionKind::Plain)
}

fn is_plain_option_label(label: &str) -> bool {
    let lower = label.to_lowercase();
    !label.is_empty()
        && !matches!(
            lower.as_str(),
            "search"
                | "filter:"
                | "popular"
                | "popular providers"
                | "providers"
                | "ask"
                | "buil"
                | "build"
        )
        && !lower.contains(" of ")
        && !lower.starts_with("type to ")
        && !lower.starts_with("view all ")
        && !lower.starts_with("use /")
        && !lower.starts_with("cursor agent")
        && !lower.starts_with('v')
}

fn display_label(text: &str) -> String {
    let mut label = text
        .split("          commands")
        .next()
        .unwrap_or(text)
        .trim();
    label = label
        .split(" (Tab to modify)")
        .next()
        .unwrap_or(label)
        .trim();
    if let Some((name, meta)) = split_columns(label) {
        if is_cursor_model_metadata(meta) {
            label = name;
        }
    }
    label.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_columns(text: &str) -> Option<(&str, &str)> {
    let idx = text.find("  ")?;
    Some((text[..idx].trim_end(), text[idx..].trim()))
}

fn is_cursor_model_metadata(text: &str) -> bool {
    text.contains("K ")
        || text.ends_with('K')
        || matches!(
            text,
            "Medium" | "High" | "Extra High" | "Low" | "Fast" | "Slow"
        )
}

fn strip_selected_glyph(text: &str) -> &str {
    ["❯", "›", ">", "→", "➜", "▸", "▶", "●", "•", "-", "*"]
        .iter()
        .find_map(|glyph| text.strip_prefix(glyph).map(str::trim_start))
        .unwrap_or(text)
}

fn strip_marker_prefix(text: &str) -> String {
    let mut s = strip_selected_glyph(text.trim_start()).to_string();
    for prefix in ["[ ]", "[x]", "[X]", "( )", "(x)", "(X)", "☐", "☑", "☒"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim_start().to_string();
            break;
        }
    }
    if let Some((_, rest)) = shortcut_token_and_rest(&s) {
        return rest.trim_start().to_string();
    }
    if let Some((_, rest)) = marker_token_and_rest(&s) {
        return rest.trim_start().to_string();
    }
    s
}

fn marker_class(text: &str) -> MarkerClass {
    if contains_checkbox(text) {
        return MarkerClass::Checkbox;
    }
    if shortcut_token(text).is_some() {
        return MarkerClass::KeyShortcut;
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
    marker_token_and_rest(text).map(|(token, _)| token)
}

fn marker_token_and_rest(text: &str) -> Option<(&str, &str)> {
    let marker = text.find(['.', ')'])?;
    let rest = text.get(marker + 1..)?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let token = text[..marker].trim();
    if !token.is_empty() && token.len() <= 3 {
        Some((token, rest))
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
        || lower.contains("←/→")
        || lower.contains("left/right")
}

fn is_box_border(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && (trimmed.chars().filter(|c| is_border_char(*c)).count() * 2 >= trimmed.chars().count()
            || trimmed.chars().filter(|c| is_border_char(*c)).count() >= 6)
}

fn is_vertical_border(c: char) -> bool {
    matches!(c, '│' | '┃' | '║')
}

#[cfg(test)]
mod tests;
