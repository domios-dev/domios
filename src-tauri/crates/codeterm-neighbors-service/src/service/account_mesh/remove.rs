use std::collections::{BTreeMap, HashMap};

use codeterm_account::account_mesh::contracts::AccountMeshOutcome;
use codeterm_account::account_mesh::wire::{MeshManagementRow, MeshTarget, RegistryPresence};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RemovedResultView {
    pub installation_id: String,
    pub display_name: Option<String>,
    pub slot_revision: u64,
    pub signed_out: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AmbiguousMachineMatch {
    pub label: String,
    pub installation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoveTargetError {
    Empty,
    CannotRemoveSelf,
    NotFound(String),
    AlreadyRemoved(String),
    Ambiguous {
        query: String,
        matches: Vec<AmbiguousMachineMatch>,
    },
}

impl RemoveTargetError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "invalid_request",
            Self::CannotRemoveSelf => "cannot_remove_self",
            Self::NotFound(_) => "machine_not_found",
            Self::AlreadyRemoved(_) => "already_removed",
            Self::Ambiguous { .. } => "ambiguous_machine",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Empty => "Machine identifier cannot be empty".into(),
            Self::CannotRemoveSelf => "Cannot remove the current machine from the mesh".into(),
            Self::NotFound(query) => format!("Machine '{query}' not found in the account mesh"),
            Self::AlreadyRemoved(query) => {
                format!("Machine '{query}' is already removed from the account mesh")
            }
            Self::Ambiguous { query, matches } => {
                let choices: Vec<String> = matches
                    .iter()
                    .map(|m| format!("{} ({})", m.label, m.installation_id))
                    .collect();
                format!(
                    "Ambiguous machine '{query}'. Specify the label or installation ID: {}",
                    choices.join(", ")
                )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountMeshRemoveOutcome {
    Outcome(AccountMeshOutcome<RemovedResultView>),
    Target(RemoveTargetError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRemoveTarget {
    pub row: MeshManagementRow,
    pub target: MeshTarget,
    pub label: String,
}

/// Strip bracketed instance suffixes (e.g. `BLACKSTAR [dev]` -> `BLACKSTAR`), matching the UI's
/// `cleanMachineName` helper so disambiguated labels (`BLACKSTAR #4`) match the Mesh Inventory UI.
pub fn clean_machine_name(raw: Option<&str>, peer_id: &str) -> Option<String> {
    let raw = raw?;
    let mut cleaned = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => cleaned.push(ch),
            _ => {}
        }
    }
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || trimmed == peer_id || is_uuid(trimmed) {
        return None;
    }
    Some(trimmed.to_owned())
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (idx, &b) in bytes.iter().enumerate() {
        let hyphen = matches!(idx, 8 | 13 | 18 | 23);
        if hyphen {
            if b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// Computes the same disambiguated display labels (`Name #1`, `Name #2`, ...) as the UI.
pub fn disambiguated_labels(rows: &[MeshManagementRow]) -> HashMap<String, String> {
    let mut labels = HashMap::new();
    let mut groups: BTreeMap<String, Vec<&MeshManagementRow>> = BTreeMap::new();
    let mut seen = HashMap::new();
    for row in rows {
        seen.insert(row.installation_id.as_str(), row);
    }
    for row in seen.values() {
        if !row.active {
            continue;
        }
        let Some(name) = clean_machine_name(row.display_name.as_deref(), &row.installation_id)
        else {
            continue;
        };
        labels.insert(row.installation_id.clone(), name.clone());
        groups.entry(name).or_default().push(*row);
    }
    for (name, mut members) in groups {
        if members.len() < 2 {
            continue;
        }
        members.sort_by(|a, b| {
            a.admitted_at
                .cmp(&b.admitted_at)
                .then_with(|| a.installation_id.cmp(&b.installation_id))
        });
        for (idx, row) in members.into_iter().enumerate() {
            labels.insert(row.installation_id.clone(), format!("{name} #{}", idx + 1));
        }
    }
    labels
}

/// Returns true when the target machine is currently online (connected or holding a live lease and endpoint).
pub fn is_row_online(row: &MeshManagementRow, connected_live: bool, now: u64) -> bool {
    connected_live
        || (row.active
            && row.presence == RegistryPresence::Present
            && row.membership_expires_at.is_some_and(|at| at > now)
            && row.endpoint.as_ref().is_some_and(|ep| ep.expires_at > now))
}

/// Resolves a user-supplied machine selector against account registry rows and live pair IDs.
pub fn resolve_remove_target(
    query: &str,
    local_installation_id: &str,
    local_instance_id: Option<&str>,
    rows: &[MeshManagementRow],
    pair_instances: &[(String, String)],
) -> Result<ResolvedRemoveTarget, RemoveTargetError> {
    let needle = query.trim();
    if needle.is_empty() {
        return Err(RemoveTargetError::Empty);
    }
    if needle == local_installation_id
        || local_instance_id.is_some_and(|id| !id.is_empty() && id == needle)
    {
        return Err(RemoveTargetError::CannotRemoveSelf);
    }

    let labels = disambiguated_labels(rows);
    let active_rows: Vec<&MeshManagementRow> = rows.iter().filter(|row| row.active).collect();
    let label_for = |row: &MeshManagementRow| -> String {
        labels
            .get(&row.installation_id)
            .cloned()
            .or_else(|| row.display_name.clone())
            .unwrap_or_else(|| row.installation_id.clone())
    };

    // 1. Exact installation_id or paired instance_id match.
    if let Some(row) = rows.iter().find(|row| row.installation_id == needle) {
        if !row.active {
            return Err(RemoveTargetError::AlreadyRemoved(needle.to_owned()));
        }
        return Ok(ResolvedRemoveTarget {
            target: row.target(),
            label: label_for(row),
            row: row.clone(),
        });
    }
    if let Some((_, installation_id)) = pair_instances
        .iter()
        .find(|(instance_id, _)| instance_id == needle)
    {
        if installation_id == local_installation_id {
            return Err(RemoveTargetError::CannotRemoveSelf);
        }
        if let Some(row) = active_rows
            .iter()
            .find(|row| row.installation_id == *installation_id)
        {
            return Ok(ResolvedRemoveTarget {
                target: row.target(),
                label: label_for(row),
                row: (*row).clone(),
            });
        }
    }

    // 2. Exact match on disambiguated label or raw display_name across all active rows (including self).
    let match_rows = |pred: &dyn Fn(&MeshManagementRow) -> bool| -> Vec<&MeshManagementRow> {
        active_rows
            .iter()
            .copied()
            .filter(|row| pred(row))
            .collect()
    };

    let exact_label = match_rows(&|row| {
        labels
            .get(&row.installation_id)
            .is_some_and(|l| l == needle)
    });
    let candidates = if !exact_label.is_empty() {
        exact_label
    } else {
        let exact_raw = match_rows(&|row| row.display_name.as_deref() == Some(needle));
        if !exact_raw.is_empty() {
            exact_raw
        } else {
            let exact_clean = match_rows(&|row| {
                clean_machine_name(row.display_name.as_deref(), &row.installation_id).as_deref()
                    == Some(needle)
            });
            if !exact_clean.is_empty() {
                exact_clean
            } else {
                let lower = needle.to_lowercase();
                let ci_label = match_rows(&|row| {
                    labels
                        .get(&row.installation_id)
                        .is_some_and(|l| l.to_lowercase() == lower)
                });
                if !ci_label.is_empty() {
                    ci_label
                } else {
                    match_rows(&|row| {
                        row.display_name
                            .as_deref()
                            .is_some_and(|name| name.to_lowercase() == lower)
                            || clean_machine_name(row.display_name.as_deref(), &row.installation_id)
                                .is_some_and(|name| name.to_lowercase() == lower)
                    })
                }
            }
        }
    };

    match candidates.as_slice() {
        [] => Err(RemoveTargetError::NotFound(needle.to_owned())),
        [only] => {
            if only.installation_id == local_installation_id {
                Err(RemoveTargetError::CannotRemoveSelf)
            } else {
                Ok(ResolvedRemoveTarget {
                    target: only.target(),
                    label: label_for(only),
                    row: (*only).clone(),
                })
            }
        }
        many => {
            let matches = many
                .iter()
                .filter(|row| row.installation_id != local_installation_id)
                .map(|row| AmbiguousMachineMatch {
                    label: label_for(row),
                    installation_id: row.installation_id.clone(),
                })
                .collect();
            Err(RemoveTargetError::Ambiguous {
                query: needle.to_owned(),
                matches,
            })
        }
    }
}

#[cfg(test)]
mod tests;
