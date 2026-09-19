use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const EXEMPTIONS: [&str; 3] = ["begin_here.md", "capabilities.md", "core.md"];

pub(super) fn validate(root: &Path, node_ids: &BTreeSet<String>) -> Result<(), String> {
    for path in stable_documents(root)? {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let name = format!("essences/{file_name}");
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read stable essence {name}: {error}"))?;
        let blocks = map_blocks(&text).map_err(|error| format!("{name}: {error}"))?;
        let block = match blocks.as_slice() {
            [] => {
                return Err(format!(
                    "stable essence {name} is missing a fenced `map` block"
                ))
            }
            [block] => block,
            _ => {
                return Err(format!(
                    "stable essence {name} has multiple fenced `map` blocks"
                ))
            }
        };
        let anchors = parse_anchors(block).map_err(|error| format!("{name}: {error}"))?;
        for anchor in anchors {
            if !node_ids.contains(anchor) {
                return Err(format!(
                    "stable essence {name} has unknown map anchor `{anchor}`"
                ));
            }
        }
    }
    Ok(())
}

fn stable_documents(root: &Path) -> Result<Vec<PathBuf>, String> {
    let directory = root.join("essences");
    let entries = fs::read_dir(&directory).map_err(|error| {
        format!(
            "cannot read stable essences in {}: {error}",
            directory.display()
        )
    })?;
    let mut documents = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot list stable essences: {error}"))?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if entry
            .file_type()
            .map_err(|error| format!("cannot inspect stable essence {}: {error}", path.display()))?
            .is_file()
            && path.extension().is_some_and(|extension| extension == "md")
            && !EXEMPTIONS.contains(&file_name)
        {
            documents.push(path);
        }
    }
    documents.sort();
    Ok(documents)
}

fn map_blocks(text: &str) -> Result<Vec<Vec<&str>>, String> {
    let mut open: Option<(char, usize, bool, Vec<&str>)> = None;
    let mut blocks = Vec::new();
    for line in text.lines() {
        if open
            .as_ref()
            .is_some_and(|(marker, length, _, _)| closes_fence(line, *marker, *length))
        {
            if let Some((_, _, is_map, contents)) = open.take() {
                if is_map {
                    blocks.push(contents);
                }
            }
            continue;
        }
        if let Some((_, _, is_map, contents)) = open.as_mut() {
            if *is_map {
                contents.push(line);
            }
        } else if let Some((marker, length, info)) = fence_parts(line) {
            open = Some((marker, length, info.trim() == "map", Vec::new()));
        }
    }
    if open.as_ref().is_some_and(|(_, _, is_map, _)| *is_map) {
        return Err("unterminated fenced `map` block".to_string());
    }
    Ok(blocks)
}

fn fence_parts(line: &str) -> Option<(char, usize, &str)> {
    let line = line.trim_start();
    let marker = line.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let length = line
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (length >= 3).then(|| (marker, length, &line[length..]))
}

fn closes_fence(line: &str, marker: char, length: usize) -> bool {
    fence_parts(line).is_some_and(|(found, found_length, info)| {
        found == marker && found_length >= length && info.trim().is_empty()
    })
}

fn parse_anchors<'a>(block: &[&'a str]) -> Result<Vec<&'a str>, String> {
    let mut in_anchors = false;
    let mut anchors = Vec::new();
    for &line in block {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "anchors:" && !in_anchors {
            in_anchors = true;
            continue;
        }
        let Some(anchor) = line.strip_prefix("- ") else {
            return Err("map block must contain one `anchors:` list".to_string());
        };
        if !in_anchors || anchor.trim().is_empty() {
            return Err("map block contains an invalid `anchors:` item".to_string());
        }
        anchors.push(anchor.trim());
    }
    if !in_anchors {
        return Err("map block is missing its `anchors:` list".to_string());
    }
    if anchors.is_empty() {
        return Err("map block has an empty `anchors:` list".to_string());
    }
    Ok(anchors)
}
