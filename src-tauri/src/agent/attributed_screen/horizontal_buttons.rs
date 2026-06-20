use std::collections::BTreeMap;

use super::picker_helpers::is_border_char;
use super::{
    cell_char, find_question, AttributedCell, AttributedColor, AttributedScreen, PickerDetection,
    RowFeature,
};

struct RowSegment {
    label: String,
    highlighted: bool,
}

pub(super) fn detect_horizontal_button_row(
    screen: &AttributedScreen,
    rows: &[RowFeature],
) -> Option<PickerDetection> {
    let mut best: Option<(usize, PickerDetection)> = None;
    for (idx, row) in rows.iter().enumerate() {
        if row.footer || row.border || row.clean.is_empty() {
            continue;
        }
        let Some((segments, selected)) = horizontal_button_segments(screen, row.row) else {
            continue;
        };
        if segments.len() < 2 {
            continue;
        }
        let footer_near = rows[idx + 1..rows.len().min(idx + 6)]
            .iter()
            .any(|r| r.footer);
        let cursor_near = screen.cursor_row.abs_diff(row.row) <= 1;
        if !footer_near && !cursor_near {
            continue;
        }
        let question = find_question(rows, idx)?;
        let options: Vec<String> = segments.into_iter().map(|seg| seg.label).collect();
        let score = options.len() + usize::from(footer_near) * 5 + usize::from(cursor_near) * 2;
        let detection = PickerDetection {
            question,
            options,
            selected_index: selected,
            multi_select: false,
        };
        if best.as_ref().is_none_or(|(prev, _)| score > *prev) {
            best = Some((score, detection));
        }
    }
    best.map(|(_, detection)| detection)
}

fn horizontal_button_segments(
    screen: &AttributedScreen,
    row: u16,
) -> Option<(Vec<RowSegment>, usize)> {
    let cells = screen.cells.get(row as usize)?;
    let (panel_bg, highlight_bg) = button_row_palette(screen, cells)?;

    let mut segments: Vec<RowSegment> = Vec::new();
    let mut current: Option<RowSegment> = None;
    let mut gap = 0usize;

    for cell in cells.iter() {
        let ch = cell_char(cell);
        let highlighted = cell_is_highlight(cell, &highlight_bg);
        let on_panel = cell.bg == panel_bg || highlighted;
        let blank = ch.trim().is_empty();
        if blank || !on_panel || is_border_char(ch.chars().next().unwrap_or(' ')) {
            gap += 1;
            if gap >= 2 {
                if let Some(seg) = current.take() {
                    segments.push(seg);
                }
            }
            continue;
        }
        match &mut current {
            Some(seg) => {
                if gap == 1 {
                    seg.label.push(' ');
                }
                seg.label.push_str(ch);
                seg.highlighted |= highlighted;
            }
            None => {
                current = Some(RowSegment {
                    label: ch.to_string(),
                    highlighted,
                });
            }
        }
        gap = 0;
    }
    if let Some(seg) = current.take() {
        segments.push(seg);
    }

    for seg in &mut segments {
        seg.label = seg.label.trim().to_string();
    }
    segments.retain(|seg| !seg.label.is_empty() && seg.label.chars().any(|c| c.is_alphanumeric()));
    if segments.len() < 2 {
        return None;
    }
    let highlighted_count = segments.iter().filter(|seg| seg.highlighted).count();
    if highlighted_count != 1 {
        return None;
    }
    let selected = segments.iter().position(|seg| seg.highlighted)?;
    Some((segments, selected))
}

fn button_row_palette(
    screen: &AttributedScreen,
    cells: &[AttributedCell],
) -> Option<(AttributedColor, AttributedColor)> {
    let mut counts = BTreeMap::<AttributedColor, usize>::new();
    let mut inverse_present = false;
    for cell in cells {
        if cell.ch.trim().is_empty() {
            continue;
        }
        if cell.inverse {
            inverse_present = true;
        }
        if cell.bg != screen.dominant_bg && cell.bg != AttributedColor::Default && !cell.inverse {
            *counts.entry(cell.bg.clone()).or_default() += 1;
        }
    }
    let mut ranked: Vec<(AttributedColor, usize)> = counts.into_iter().collect();
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));

    if inverse_present {
        let panel = ranked
            .first()
            .map(|(bg, _)| bg.clone())
            .unwrap_or(AttributedColor::Default);
        return Some((panel, AttributedColor::Default));
    }
    let panel = ranked.first().map(|(bg, _)| bg.clone())?;
    let highlight = ranked.get(1).map(|(bg, _)| bg.clone())?;
    Some((panel, highlight))
}

fn cell_is_highlight(cell: &AttributedCell, highlight_bg: &AttributedColor) -> bool {
    if cell.inverse {
        return true;
    }
    *highlight_bg != AttributedColor::Default && cell.bg == *highlight_bg
}
