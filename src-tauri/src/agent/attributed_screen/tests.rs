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

#[test]
fn detects_horizontal_button_row_with_highlight() {
    let panel = AttributedColor::Rgb(30, 30, 30);
    let highlight = AttributedColor::Rgb(245, 167, 66);
    let theme = AttributedColor::Rgb(10, 10, 10);

    let mut cells = vec![
        row_on_bg("Access external directory /tmp", &theme),
        row_on_bg("", &theme),
        button_row(&panel, &highlight),
        row_on_bg("enter confirm", &theme),
    ];
    let cols = cells.iter().map(Vec::len).max().unwrap_or(0) as u16;
    for row in &mut cells {
        row.resize_with(cols as usize, default_cell);
    }

    let screen = AttributedScreen {
        rows: cells.len() as u16,
        cols,
        cells,
        cursor_row: 2,
        cursor_col: 6,
        dominant_bg: theme,
        dominant_fg: AttributedColor::Default,
    };

    let detected = detect_picker(&screen).expect("horizontal picker");
    assert_eq!(detected.question, "Access external directory /tmp");
    assert_eq!(detected.options, ["Allow once", "Allow always", "Reject"]);
    assert_eq!(detected.selected_index, 0);
    assert!(!detected.multi_select);
}

fn cell_on(ch: char, bg: &AttributedColor) -> AttributedCell {
    AttributedCell {
        ch: ch.to_string(),
        fg: AttributedColor::Default,
        bg: bg.clone(),
        inverse: false,
        bold: false,
    }
}

fn row_on_bg(text: &str, bg: &AttributedColor) -> Vec<AttributedCell> {
    text.chars().map(|ch| cell_on(ch, bg)).collect()
}

fn button_row(panel: &AttributedColor, highlight: &AttributedColor) -> Vec<AttributedCell> {
    let mut row = Vec::new();
    for ch in " Allow once ".chars() {
        row.push(cell_on(ch, highlight));
    }
    for ch in "  ".chars() {
        row.push(cell_on(ch, panel));
    }
    for ch in "Allow always".chars() {
        row.push(cell_on(ch, panel));
    }
    for ch in "  ".chars() {
        row.push(cell_on(ch, panel));
    }
    for ch in "Reject".chars() {
        row.push(cell_on(ch, panel));
    }
    row
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
