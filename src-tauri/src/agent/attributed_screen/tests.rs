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
