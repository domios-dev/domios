use super::{orphans::Node, shell_command};
use std::{collections::HashSet, path::Path};

pub(super) fn shell_matches(
    argv: &[String],
    cwd: &Path,
    command: &str,
    expected_cwd: &Path,
) -> bool {
    if cwd.to_str().is_none()
        || expected_cwd.to_str().is_none()
        || super::normalized_path(cwd) != super::normalized_path(expected_cwd)
    {
        return false;
    }

    const UTF8_SETUP: &str =
        "try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}";
    let Some(script) = shell_command(argv) else {
        return false;
    };
    if script == command {
        return true;
    }
    matches!(super::executable(&argv[0]).as_str(), "pwsh" | "powershell")
        && script.split_once('\n').is_some_and(|(setup, body)| {
            setup.trim_end_matches('\r') == UTF8_SETUP && body == command
        })
}

pub(super) fn subtree(
    nodes: &[Node],
    candidates: &[usize],
    protected: &HashSet<usize>,
) -> Option<Vec<usize>> {
    let [root] = candidates else {
        return None;
    };
    if nodes[*root].infrastructure || nodes[*root].exited {
        return None;
    }

    let mut branch = vec![*root];
    let mut pids = HashSet::from([nodes[*root].pid]);
    let mut cursor = 0;
    while let Some(&parent) = branch.get(cursor) {
        for (position, node) in nodes.iter().enumerate() {
            if node.parent == nodes[parent].pid && pids.insert(node.pid) {
                branch.push(position);
            }
        }
        cursor += 1;
    }
    branch
        .iter()
        .all(|&position| !nodes[position].infrastructure && !protected.contains(&position))
        .then_some(branch)
}

#[cfg(test)]
mod tests;
