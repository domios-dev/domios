//! Body locators: file + 1-based line range of an entity's markdown body
//! inside its yaml, so agents can edit bodies in place with standard tools.

use super::{PlanError, PlanStore};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BodyLocation {
    pub file: PathBuf,
    /// First line of the body block content (the line after `body: |-`),
    /// or the `body:` line itself when the body is empty/inline.
    pub start_line: usize,
    pub end_line: usize,
    pub editable: bool,
}

pub enum LocateTarget<'a> {
    Task(&'a str),
    PlanSection(&'a str),
    SpecSection(&'a str),
}

impl PlanStore {
    pub fn locate_body(
        &self,
        plan_id: &str,
        target: LocateTarget<'_>,
    ) -> Result<BodyLocation, PlanError> {
        #[cfg(test)]
        {
            let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
            let root = super::paths::test_project_root(super::paths::plan_sidecar_root(&plan))
                .ok_or(PlanError::NotFound)?;
            let dir = super::store_v2::plan_state_dir(&plan)
                .map_err(|error| PlanError::Io(error.to_string()))?;
            let (file, range) = match target {
                LocateTarget::Task(id) => {
                    let task = plan
                        .tasks
                        .iter()
                        .find(|task| task.id == id)
                        .ok_or(PlanError::NotFound)?;
                    let file = super::store_v2::task_yaml_path(&dir, task);
                    let text = std::fs::read_to_string(&file)
                        .map_err(|error| PlanError::Io(error.to_string()))?;
                    (file, top_level_body_range(&text))
                }
                LocateTarget::PlanSection(id) => {
                    let file = super::store_v2::plan_yaml_path(&dir);
                    let text = std::fs::read_to_string(&file)
                        .map_err(|error| PlanError::Io(error.to_string()))?;
                    (file, section_body_range(&text, id))
                }
                LocateTarget::SpecSection(id) => {
                    let file = dir.join("spec.yaml");
                    let text = std::fs::read_to_string(&file)
                        .map_err(|error| PlanError::Io(error.to_string()))?;
                    (file, section_body_range(&text, id))
                }
            };
            let (start_line, end_line, editable) = range.ok_or(PlanError::NotFound)?;
            let _ = root;
            Ok(BodyLocation {
                file,
                start_line,
                end_line,
                editable,
            })
        }
        #[cfg(not(test))]
        {
            let _ = (plan_id, target);
            Err(PlanError::Io(
                "plan bodies are database-backed; use plan mutation commands".into(),
            ))
        }
    }
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn is_block_scalar(value: &str) -> bool {
    matches!(value.trim(), "|" | "|-" | "|+" | ">" | ">-" | ">+")
}

/// Range of the block content following `key: |` at line `key_idx` (0-based).
/// Returns 1-based inclusive (start, end); block lines are empty or deeper-indented.
fn block_range(lines: &[&str], key_idx: usize, key_indent: usize) -> (usize, usize) {
    let mut end = key_idx;
    for (i, line) in lines.iter().enumerate().skip(key_idx + 1) {
        if line.trim().is_empty() || indent_of(line) > key_indent {
            end = i;
        } else {
            break;
        }
    }
    if end == key_idx {
        // Empty block: point at the key line itself.
        (key_idx + 1, key_idx + 1)
    } else {
        (key_idx + 2, end + 1)
    }
}

/// Body range for a single-entity yaml (task file): the top-level `body:` key.
/// Returns (start, end, editable) — editable=false for inline/null bodies.
pub fn top_level_body_range(text: &str) -> Option<(usize, usize, bool)> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if indent_of(line) == 0 {
            if let Some(value) = line.strip_prefix("body:") {
                if is_block_scalar(value) {
                    let (s, e) = block_range(&lines, i, 0);
                    return Some((s, e, s > i + 1));
                }
                return Some((i + 1, i + 1, false));
            }
        }
    }
    None
}

/// Body range for the section with `sec_id` inside a plan/spec yaml. Anchors on
/// the section's `id:` line, widens to the whole list-item window (field order
/// varies: serde emits id first, the python migration wrote body before id),
/// then finds the `body:` field at the item's mapping indent.
pub fn section_body_range(text: &str, sec_id: &str) -> Option<(usize, usize, bool)> {
    let lines: Vec<&str> = text.lines().collect();
    let id_plain = format!("id: {sec_id}");
    let id_dashed = format!("- id: {sec_id}");
    let anchor = lines.iter().position(|line| {
        let t = line.trim();
        t == id_plain || t == id_dashed
    })?;
    let anchor_is_dash = lines[anchor].trim_start().starts_with("- ");
    let field_indent = if anchor_is_dash {
        indent_of(lines[anchor]) + 2
    } else {
        indent_of(lines[anchor])
    };
    let dash_indent = field_indent - 2;

    // Walk back to the `- ` line that opens this list item.
    let mut item_start = anchor;
    if !anchor_is_dash {
        for i in (0..anchor).rev() {
            let line = lines[i];
            if line.trim().is_empty() {
                continue;
            }
            let ind = indent_of(line);
            if ind == dash_indent && line.trim_start().starts_with("- ") {
                item_start = i;
                break;
            }
            if ind < dash_indent {
                break;
            }
        }
    }

    for (i, line) in lines.iter().enumerate().skip(item_start) {
        let ind = indent_of(line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Next sibling item or a shallower key ends this section's window.
        if i > item_start
            && (ind < field_indent || (ind == dash_indent && trimmed.starts_with("- ")))
        {
            break;
        }
        // A field either sits at field_indent or rides the item's dash line.
        let field = if i == item_start && trimmed.starts_with("- ") {
            trimmed.trim_start_matches("- ")
        } else if ind == field_indent && !trimmed.starts_with("- ") {
            trimmed
        } else {
            continue;
        };
        if let Some(value) = field.strip_prefix("body:") {
            if is_block_scalar(value) {
                let (s, e) = block_range(&lines, i, field_indent);
                return Some((s, e, s > i + 1));
            }
            return Some((i + 1, i + 1, false));
        }
    }
    None
}

#[cfg(test)]
mod tests;
