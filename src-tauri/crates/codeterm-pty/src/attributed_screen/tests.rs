use super::*;

#[test]
fn captured_block_wide_picker_prefers_marker_column() {
    let screen = screen_from_capture(include_str!("fixtures/block_wide_picker.b64"));

    let option_rows = 30..=36;
    let block_background = AttributedColor::Rgb(41, 41, 41);
    assert_eq!(screen.cells[33][0].ch, "›");
    assert!(option_rows
        .clone()
        .all(|row| screen.cells[row][0].bg == block_background));
    assert!(option_rows
        .clone()
        .all(|row| screen.cells[row][239].bg == block_background));

    let detected = detect_picker(&screen).expect("picker");

    assert_eq!(detected.selected_index, 3);
}

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

#[test]
fn distinguishing_background_still_selects_one_row() {
    let mut screen = screen_from_rows(&[
        ("Choose an option", false),
        ("  First", false),
        ("  Second", false),
        ("  Third", true),
        ("  Fourth", false),
        ("Enter to select", false),
    ]);
    screen.cursor_row = 0;

    let detected = detect_picker(&screen).expect("picker");

    assert_eq!(detected.selected_index, 2);
}

#[test]
fn uniform_option_background_is_not_row_selection() {
    let mut screen = screen_from_rows(&[
        ("Choose an option", false),
        ("  First", true),
        ("  Second", true),
        ("  Third", true),
        ("Enter to select", false),
    ]);
    screen.cursor_row = 0;

    let detected = detect_picker(&screen).expect("picker");

    assert_eq!(detected.selected_index, 0);
}

#[test]
fn title_search_panel_marker_wins_uniform_background() {
    let mut screen = screen_from_rows(&[
        ("Choose an option esc", false),
        ("Search", false),
        ("  First", true),
        ("  Second", true),
        ("› Third", true),
        ("  Fourth", true),
    ]);
    screen.cursor_row = 0;

    let detected = detect_picker(&screen).expect("picker");

    assert_eq!(detected.selected_index, 2);
}

#[test]
fn captured_title_search_panel_ignores_side_band_backgrounds() {
    let screen = screen_from_cast(include_str!(
        "../../../../../testbench/snapshots/opencode/connect-picker/raw.cast"
    ));

    let detected = detect_picker(&screen).expect("picker");

    assert_eq!(detected.selected_index, 0);
}

fn screen_from_capture(encoded: &str) -> AttributedScreen {
    use base64::Engine;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .expect("valid captured frame");
    let mut parser = vt100::Parser::new(40, 240, 0);
    parser.process(&bytes);
    AttributedScreen::from_vt100(parser.screen())
}

fn screen_from_cast(cast: &str) -> AttributedScreen {
    let mut lines = cast.lines();
    let header: serde_json::Value = serde_json::from_str(lines.next().expect("cast header"))
        .expect("valid cast header");
    let rows = header["height"].as_u64().expect("cast height") as u16;
    let cols = header["width"].as_u64().expect("cast width") as u16;
    let mut parser = vt100::Parser::new(rows, cols, 0);
    for line in lines {
        let event: serde_json::Value = serde_json::from_str(line).expect("valid cast event");
        if event[1] == "o" {
            parser.process(event[2].as_str().expect("cast output").as_bytes());
        }
    }
    AttributedScreen::from_vt100(parser.screen())
}
