use crate::agent::signal_extractor::StreamSignals;
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use codeterm_pty::attributed_screen::{detect_picker, AttributedColor, AttributedScreen};

pub use codeterm_pty::native_prompt::NativePromptInput;

pub fn pending_prompt_from_input(input: &NativePromptInput) -> Option<PendingPrompt> {
    let detection = detect_picker(&input.screen)?;
    if !has_structural_picker_signal(input, &detection.options) {
        return None;
    }
    Some(pending_prompt_from_detection(detection))
}

/// Live-path resolution for a tab's pending-prompt card.
/// A structured handle (push/API) wins; otherwise the structurally gated
/// attributed-grid detector extracts the picker from the tab's own screen.
pub fn resolve_pending_prompt(
    structured: Option<PendingPrompt>,
    input: Option<&NativePromptInput>,
) -> Option<PendingPrompt> {
    structured.or_else(|| input.and_then(pending_prompt_from_input))
}

pub fn pending_prompt_from_signals(
    screen: AttributedScreen,
    signals: &StreamSignals,
) -> Option<PendingPrompt> {
    pending_prompt_from_input(&NativePromptInput {
        screen,
        cursor_hidden: signals.input_mode.cursor_hidden,
        alt_screen: signals.input_mode.alt_screen,
    })
}

fn pending_prompt_from_detection(
    detection: codeterm_pty::attributed_screen::PickerDetection,
) -> PendingPrompt {
    let selected_value = option_value(detection.selected_index);
    PendingPrompt {
        question: detection.question,
        options: detection
            .options
            .into_iter()
            .enumerate()
            .map(|(idx, label)| PromptOption {
                label,
                value: option_value(idx),
            })
            .collect(),
        sections: None,
        multi_select: Some(detection.multi_select),
        submit_available: Some(!detection.multi_select),
        selected_values: Some(vec![selected_value]),
    }
}

fn option_value(index: usize) -> String {
    (index + 1).to_string()
}

fn has_structural_picker_signal(input: &NativePromptInput, options: &[String]) -> bool {
    input.cursor_hidden
        || input.alt_screen
        || has_picker_footer(&input.screen)
        || has_selection_attribute(&input.screen, options)
}

fn has_picker_footer(screen: &AttributedScreen) -> bool {
    (0..screen.rows).any(|row| is_picker_footer(&screen.row_text(row)))
}

fn is_picker_footer(text: &str) -> bool {
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

fn has_selection_attribute(screen: &AttributedScreen, options: &[String]) -> bool {
    options
        .iter()
        .any(|option| attributed_row_for_option(screen, option).is_some())
}

fn attributed_row_for_option(screen: &AttributedScreen, option: &str) -> Option<u16> {
    if option.trim().is_empty() {
        return None;
    }
    (0..screen.rows)
        .filter(|&row| screen.row_text(row).contains(option))
        .find(|&row| row_has_selection_attribute(screen, row))
}

fn row_has_selection_attribute(screen: &AttributedScreen, row: u16) -> bool {
    let Some(cells) = screen.cells.get(row as usize) else {
        return false;
    };
    let mut inverse = 0usize;
    let mut bg_delta = 0usize;
    for cell in cells {
        if cell.ch.trim().is_empty() {
            continue;
        }
        if cell.inverse {
            inverse += 1;
        }
        if cell.bg != screen.dominant_bg && cell.bg != AttributedColor::Default {
            bg_delta += 1;
        }
    }
    inverse > 0 || bg_delta >= 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::signal_extractor::{SignalExtractor, VolatileHint};
    use serde_json::Value;
    use std::path::{Path, PathBuf};

    #[test]
    fn live_native_prompt_covers_picker_fixtures() {
        for case in [
            "cursor/model-picker",
            "cursor/multi-step-wizard",
            "claude/ask-user-question-wizard",
            "opencode/model-picker",
            "opencode/connect-picker",
        ] {
            let dir = snapshots_root().join(case);
            let prompt = prompt_from_cast(&dir).unwrap_or_else(|| panic!("{case} prompt"));
            let expected = expected(&dir);
            assert_eq!(prompt.question, expected["question"]);
            assert_eq!(
                prompt
                    .options
                    .iter()
                    .map(|option| option.label.as_str())
                    .collect::<Vec<_>>(),
                expected["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect::<Vec<_>>(),
                "{case}"
            );
            assert_eq!(
                prompt.multi_select,
                expected["multiSelect"].as_bool(),
                "{case}"
            );
            assert_eq!(
                prompt.selected_values,
                Some(vec![
                    (expected["selectedIndex"].as_u64().unwrap() + 1).to_string()
                ]),
                "{case}"
            );
        }
    }

    #[test]
    fn prose_questionnaire_has_no_native_pending_prompt() {
        let screen = prose_questionnaire_screen();
        let input = NativePromptInput {
            screen,
            cursor_hidden: false,
            alt_screen: false,
        };
        assert!(pending_prompt_from_input(&input).is_none());
    }

    #[test]
    fn captured_model_picker_frames_pass_native_fingerprint() {
        // Stage probe: were the structural fingerprint the dropping stage,
        // this would fail instead of the live-resolution oracle below.
        for case in ["claude/model-picker-r3", "codex/model-picker-r3"] {
            let dir = snapshots_root().join(case);
            let prompt = prompt_from_cast(&dir).unwrap_or_else(|| {
                panic!("{case}: native detector must accept the captured /model frame")
            });
            let expected = expected(&dir);
            assert_eq!(prompt.question, expected["question"], "{case}");
            assert_eq!(
                prompt
                    .options
                    .iter()
                    .map(|option| option.label.as_str())
                    .collect::<Vec<_>>(),
                expected["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect::<Vec<_>>(),
                "{case}"
            );
            assert_eq!(prompt.multi_select, expected["multiSelect"].as_bool(), "{case}");
            assert_eq!(
                prompt.selected_values,
                Some(vec![
                    (expected["selectedIndex"].as_u64().unwrap() + 1).to_string()
                ]),
                "{case}"
            );
        }
    }

    #[test]
    fn live_resolution_surfaces_native_picker_for_captured_model_frames() {
        // Stage oracle: the live path (output.rs → resolve_pending_prompt) must
        // surface a picker the native detector already accepts when no
        // structured handle produced one.
        for case in ["claude/model-picker-r3", "codex/model-picker-r3"] {
            let dir = snapshots_root().join(case);
            let input = input_from_cast(&dir);
            let native = pending_prompt_from_input(&input).unwrap_or_else(|| {
                panic!("{case}: precondition — the fingerprint accepts this frame")
            });
            assert_eq!(
                resolve_pending_prompt(None, Some(&input)),
                Some(native),
                "{case}: live resolution dropped a detector-accepted picker"
            );
        }
    }

    #[test]
    fn structured_prompt_wins_over_native_detection() {
        let input = input_from_cast(&snapshots_root().join("claude/model-picker-r3"));
        let structured = PendingPrompt {
            question: "structured handle".to_string(),
            options: Vec::new(),
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        };
        assert_eq!(
            resolve_pending_prompt(Some(structured.clone()), Some(&input)),
            Some(structured)
        );
    }

    #[test]
    fn chrome_and_prose_frames_yield_no_native_pending_prompt() {
        for case in [
            "claude/prose-questionnaire-negative-r1",
            "claude/prose_negative_r2",
            "claude/agent-state-turn_complete_real",
            "codex/prose-negative-r1",
            "codex/prose-negative-r2",
            "codex/prose-questionnaire-negative",
            "codex/agent-state-turn_complete_real",
        ] {
            let dir = snapshots_root().join(case);
            let input = input_from_cast(&dir);
            assert!(
                pending_prompt_from_input(&input).is_none(),
                "{case}: chrome/prose frame must not become a pending-prompt card"
            );
        }
    }

    #[test]
    fn harvest_r2_detect_picker_dump() {
        let dir = snapshots_root().join("claude/multi-step-wizard-r2");
        let mut extractor = extractor_for_cast(&dir);
        for event in cast_events(&dir) {
            match event {
                CastEvent::Output { bytes, now_ms } => {
                    extractor.step(&bytes, now_ms);
                }
                CastEvent::Resize { rows, cols } => extractor.resize(rows, cols),
            }
        }
        let signals = extractor.step(b"", 60_000);
        let screen = extractor.attributed_screen();
        let detection = detect_picker(&screen);
        let pending = pending_prompt_from_signals(screen.clone(), &signals);
        println!(
            "HARVEST_DETECT_PICKER_JSON={}",
            serde_json::to_string(&detection).unwrap()
        );
        println!(
            "HARVEST_PENDING_PROMPT_JSON={}",
            serde_json::to_string(&pending).unwrap()
        );
        let mut flat = String::new();
        for r in 0..screen.rows {
            flat.push_str(screen.row_text(r).trim_end());
            flat.push('\n');
        }
        println!("HARVEST_SCREEN_BEGIN\n{}\nHARVEST_SCREEN_END", flat);
    }

    fn prompt_from_cast(dir: &Path) -> Option<PendingPrompt> {
        pending_prompt_from_input(&input_from_cast(dir))
    }

    fn input_from_cast(dir: &Path) -> NativePromptInput {
        let mut extractor = extractor_for_cast(dir);
        for event in cast_events(dir) {
            match event {
                CastEvent::Output { bytes, now_ms } => {
                    extractor.step(&bytes, now_ms);
                }
                CastEvent::Resize { rows, cols } => extractor.resize(rows, cols),
            }
        }
        let signals = extractor.step(b"", 60_000);
        NativePromptInput {
            screen: extractor.attributed_screen(),
            cursor_hidden: signals.input_mode.cursor_hidden,
            alt_screen: signals.input_mode.alt_screen,
        }
    }

    fn extractor_for_cast(dir: &Path) -> SignalExtractor {
        let header = cast_header(dir);
        SignalExtractor::new(
            header["height"].as_u64().unwrap_or(24) as u16,
            header["width"].as_u64().unwrap_or(80) as u16,
            VolatileHint::default(),
        )
    }

    fn expected(dir: &Path) -> Value {
        serde_json::from_str::<Value>(&std::fs::read_to_string(dir.join("expected.json")).unwrap())
            .unwrap()["expected"]
            .clone()
    }

    fn cast_header(dir: &Path) -> Value {
        let text = std::fs::read_to_string(dir.join("raw.cast")).unwrap();
        serde_json::from_str(text.lines().next().unwrap()).unwrap()
    }

    enum CastEvent {
        Output { bytes: Vec<u8>, now_ms: u64 },
        Resize { rows: u16, cols: u16 },
    }

    fn cast_events(dir: &Path) -> Vec<CastEvent> {
        let text = std::fs::read_to_string(dir.join("raw.cast")).unwrap();
        text.lines()
            .skip(1)
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let row: Value = serde_json::from_str(line).unwrap();
                let arr = row.as_array().unwrap();
                match arr[1].as_str().unwrap() {
                    "o" => CastEvent::Output {
                        now_ms: (arr[0].as_f64().unwrap() * 1000.0).round() as u64,
                        bytes: arr[2].as_str().unwrap().as_bytes().to_vec(),
                    },
                    "r" => {
                        let (cols, rows) = arr[2].as_str().unwrap().split_once('x').unwrap();
                        CastEvent::Resize {
                            rows: rows.parse().unwrap(),
                            cols: cols.parse().unwrap(),
                        }
                    }
                    other => panic!("unknown cast event {other}"),
                }
            })
            .collect()
    }

    fn prose_questionnaire_screen() -> AttributedScreen {
        let rows = [
            "Could you answer these before we continue?",
            "1. Which package manager do you use?",
            "2. Should CI fail on warnings?",
            "3. Do you want docs updated?",
            "",
            "> ",
        ];
        let cols = rows.iter().map(|row| row.len()).max().unwrap() as u16;
        let cells = rows
            .iter()
            .map(|row| {
                let mut cells = row
                    .chars()
                    .map(|ch| codeterm_pty::attributed_screen::AttributedCell {
                        ch: ch.to_string(),
                        fg: AttributedColor::Default,
                        bg: AttributedColor::Default,
                        inverse: false,
                        bold: false,
                    })
                    .collect::<Vec<_>>();
                cells.resize_with(cols as usize, || {
                    codeterm_pty::attributed_screen::AttributedCell {
                        ch: String::new(),
                        fg: AttributedColor::Default,
                        bg: AttributedColor::Default,
                        inverse: false,
                        bold: false,
                    }
                });
                cells
            })
            .collect();
        AttributedScreen {
            rows: rows.len() as u16,
            cols,
            cells,
            cursor_row: 5,
            cursor_col: 2,
            dominant_bg: AttributedColor::Default,
            dominant_fg: AttributedColor::Default,
        }
    }

    fn snapshots_root() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../testbench/snapshots"
        ))
    }
}
