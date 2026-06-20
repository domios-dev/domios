// SPIKE (throwaway): can we recover picker question/options/SELECTED from raw.cast via the
// vt100 ATTRIBUTED grid (bgcolor/fgcolor/inverse), where flattened text shows no marker?
// Proves/disproves the attribute-parsing mechanic on the hardest case (cursor: bg-only selection).
use codeterm_lib::agent::attributed_screen::{detect_picker, AttributedScreen};
use serde_json::Value;
use std::collections::BTreeMap;

fn load_cast(path: &str) -> (u16, u16, Vec<u8>) {
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    let w = header["width"].as_u64().unwrap_or(120) as u16;
    let h = header["height"].as_u64().unwrap_or(40) as u16;
    let mut bytes = Vec::new();
    for line in lines {
        let row: Value = serde_json::from_str(line).unwrap();
        if row[1].as_str() == Some("o") {
            bytes.extend_from_slice(row[2].as_str().unwrap_or("").as_bytes());
        }
    }
    (w, h, bytes)
}

fn color_str(c: vt100::Color) -> Option<String> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(format!("idx{i}")),
        vt100::Color::Rgb(r, g, b) => Some(format!("rgb({r},{g},{b})")),
    }
}

fn analyze(label: &str, path: &str) {
    if !std::path::Path::new(path).exists() {
        println!("\n##### {label}: NO raw.cast at {path}");
        return;
    }
    let (w, h, bytes) = load_cast(path);
    let mut parser = vt100::Parser::new(h, w, 0);
    parser.process(&bytes);
    let screen = parser.screen();
    let attributed = AttributedScreen::from_vt100(screen);
    let (rows, cols) = screen.size();

    // dominant bg across the screen (theme background)
    let mut bg_counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in 0..rows {
        for c in 0..cols {
            if let Some(cell) = screen.cell(r, c) {
                if cell.has_contents() {
                    let key = color_str(cell.bgcolor()).unwrap_or_else(|| "default".into());
                    *bg_counts.entry(key).or_default() += 1;
                }
            }
        }
    }
    let dominant_bg = bg_counts
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(k, _)| k.clone())
        .unwrap_or_else(|| "default".into());

    println!("\n##### {label}  ({path})  {rows}x{cols}  dominant_bg={dominant_bg}");
    println!(
        "  --- rows carrying a SELECTION signal (inverse run OR bg != dominant OR accent-fg) ---"
    );
    for r in 0..rows {
        let mut text = String::new();
        let mut inverse = 0usize;
        let mut bgs: BTreeMap<String, usize> = BTreeMap::new();
        let mut fgs: BTreeMap<String, usize> = BTreeMap::new();
        for c in 0..cols {
            if let Some(cell) = screen.cell(r, c) {
                let ch = cell.contents();
                text.push_str(if ch.is_empty() { " " } else { ch });
                if cell.has_contents() {
                    if cell.inverse() {
                        inverse += 1;
                    }
                    if let Some(bg) = color_str(cell.bgcolor()) {
                        *bgs.entry(bg).or_default() += 1;
                    }
                    if let Some(fg) = color_str(cell.fgcolor()) {
                        *fgs.entry(fg).or_default() += 1;
                    }
                }
            }
        }
        let trimmed = text.trim_end();
        if trimmed.trim().is_empty() {
            continue;
        }
        let distinct_bg: Vec<&String> = bgs.keys().filter(|b| **b != dominant_bg).collect();
        let signal = inverse > 0 || !distinct_bg.is_empty();
        if signal {
            println!(
                "   row{r:2} inv={inverse:2} bg!={distinct_bg:?} fg={:?} :: {:?}",
                fgs.keys().collect::<Vec<_>>(),
                trimmed.trim()
            );
        }
    }
    let detected = detect_picker(&attributed);
    if std::env::var("SPIKE_ROWS").is_ok() {
        println!("  --- rendered rows ---");
        for r in 0..rows {
            let text = attributed.row_text(r).trim_end().to_string();
            if !text.trim().is_empty() {
                println!("   row{r:2}: {:?}", text);
            }
        }
    }
    println!("  --- fusion detector ---");
    println!("   {}", serde_json::to_string_pretty(&detected).unwrap());
    if let Some(expected) = expected_prompt(path) {
        println!("  --- expected flattened parser oracle ---");
        println!("   {}", serde_json::to_string_pretty(&expected).unwrap());
    }
}

fn expected_prompt(path: &str) -> Option<Value> {
    let expected_path = std::path::Path::new(path).parent()?.join("expected.json");
    let json: Value = serde_json::from_str(&std::fs::read_to_string(expected_path).ok()?).ok()?;
    json.get("expected")
        .and_then(|expected| expected.get("parsePrompt"))
        .cloned()
}

#[test]
fn attr_spike_dump() {
    // Point at freshly-captured casts via SPIKE_CASTS="label=path,label=path"; else defaults.
    if let Ok(spec) = std::env::var("SPIKE_CASTS") {
        for entry in spec.split(',') {
            if let Some((label, path)) = entry.split_once('=') {
                analyze(label, path);
            }
        }
        return;
    }
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../testbench/snapshots");
    analyze(
        "CURSOR model-picker (bg-only selection, hardest)",
        &format!("{root}/cursor/model-picker/raw.cast"),
    );
    analyze(
        "CLAUDE options-abc",
        &format!("{root}/claude/options-abc/raw.cast"),
    );
}
