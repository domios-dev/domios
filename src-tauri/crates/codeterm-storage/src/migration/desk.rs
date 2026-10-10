use super::compare::{self, Diff};
use super::manifest::{Facts, ImportRefusal, RecoveryCandidate, SourceStatus};
use super::{
    Captured, DomainImport, IgnoredKeys, ImportDomain, ImportProvenance, LegacyDecoder,
    SourceCapture, SourceFile, SourceRole, SourceSelection,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::desk::{
    DeskLayoutNode, DeskSnapshot, MAX_DESK_CATALOGUE, MAX_DESK_LAYOUT_DEPTH, MAX_DESK_LAYOUT_NODES,
    MAX_DESK_TABS, MAX_DESK_TEAMS, MAX_DESK_WORKSPACES,
};
use rusqlite::Connection;

pub(crate) fn empty() -> DeskSnapshot {
    DeskSnapshot {
        version: "1".to_string(),
        timestamp: 0,
        active_workspace_id: None,
        workspaces: Vec::new(),
        teams: Vec::new(),
        agent_session_identities: Vec::new(),
    }
}

pub(crate) fn normalize(snapshot: &DeskSnapshot) -> DeskSnapshot {
    let mut normalized = snapshot.clone();
    normalized.agent_session_identities.sort_by(|left, right| {
        (&left.provider, &left.session_id).cmp(&(&right.provider, &right.session_id))
    });
    normalized
}

pub(crate) fn preflight(snapshot: &DeskSnapshot, facts: &mut Facts) -> Result<(), ImportRefusal> {
    let cost = snapshot
        .validate_preserving_orphan_teams()
        .map_err(|error| ImportRefusal::Invalid {
            domain: ImportDomain::Desk,
            detail: error.to_string(),
        })?;
    let tabs: usize = snapshot
        .workspaces
        .iter()
        .map(|workspace| workspace.tabs.len())
        .sum();
    let (nodes, depth, fields_bytes) = snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.tabs)
        .fold((0, 0, 0), |stats, tab| layout_stats(&tab.layout, 0, stats));
    facts.bound(
        ImportDomain::Desk,
        "workspaces",
        MAX_DESK_WORKSPACES as u64,
        snapshot.workspaces.len() as u64,
    );
    facts.bound(
        ImportDomain::Desk,
        "tabs",
        MAX_DESK_TABS as u64,
        tabs as u64,
    );
    facts.bound(
        ImportDomain::Desk,
        "teams",
        MAX_DESK_TEAMS as u64,
        snapshot.teams.len() as u64,
    );
    facts.bound(
        ImportDomain::Desk,
        "layout-nodes",
        MAX_DESK_LAYOUT_NODES as u64,
        nodes as u64,
    );
    facts.bound(
        ImportDomain::Desk,
        "layout-depth",
        MAX_DESK_LAYOUT_DEPTH as u64,
        depth as u64,
    );
    facts.bound(
        ImportDomain::Desk,
        "identity-catalogue",
        MAX_DESK_CATALOGUE as u64,
        snapshot.agent_session_identities.len() as u64,
    );
    facts.bound(
        ImportDomain::Desk,
        "largest-node-fields-bytes",
        crate::repositories::desk::MAX_DESK_LEAF_BYTES as u64,
        fields_bytes as u64,
    );
    facts.bound(
        ImportDomain::Desk,
        "mailbox-bytes",
        crate::DEFAULT_MAILBOX_BYTES as u64,
        cost as u64,
    );
    facts.count(ImportDomain::Desk, "teams", snapshot.teams.len() as u64);
    facts.count(
        ImportDomain::Desk,
        "identities",
        snapshot.agent_session_identities.len() as u64,
    );
    Ok(())
}

fn layout_stats(
    node: &DeskLayoutNode,
    depth: usize,
    mut stats: (usize, usize, usize),
) -> (usize, usize, usize) {
    stats.0 += 1;
    stats.1 = stats.1.max(depth);
    match node {
        DeskLayoutNode::Leaf { fields, .. } => {
            stats.2 = stats
                .2
                .max(serde_json::to_vec(&fields.values).map_or(usize::MAX, |bytes| bytes.len()));
        }
        DeskLayoutNode::Split {
            children, fields, ..
        } => {
            stats.2 = stats
                .2
                .max(serde_json::to_vec(fields).map_or(usize::MAX, |bytes| bytes.len()));
            for child in children {
                stats = layout_stats(child, depth + 1, stats);
            }
        }
    }
    stats
}

pub(crate) fn occupied(conn: &Connection) -> Result<bool, StorageError> {
    let occupied: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM desk_meta) OR EXISTS(SELECT 1 FROM workspaces) \
             OR EXISTS(SELECT 1 FROM desk_workspace_state) OR EXISTS(SELECT 1 FROM desk_teams) \
             OR EXISTS(SELECT 1 FROM desk_tabs) OR EXISTS(SELECT 1 FROM desk_layout_nodes) \
             OR EXISTS(SELECT 1 FROM agent_session_identities)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| sqlite_error("check desk rows before import", error))?;
    Ok(occupied)
}

pub(crate) fn write(cx: &mut WriteCx<'_>, snapshot: DeskSnapshot) -> Result<(), StorageError> {
    crate::repositories::desk::write_import(cx, snapshot)
}

pub(crate) fn verify(conn: &Connection, expected: &DeskSnapshot) -> Result<(), ImportRefusal> {
    let actual = crate::repositories::desk::import_projection(conn).map_err(|error| {
        ImportRefusal::Invalid {
            domain: ImportDomain::Desk,
            detail: error.to_string(),
        }
    })?;
    let mut diff = Diff::new(ImportDomain::Desk);
    compare::diff_desk(expected, &actual, &mut diff);
    diff.gate()
}

type Statuses = Vec<(ImportDomain, String, SourceStatus)>;

const DESK_SNAPSHOT: &str = "sessions/last.json";
const DESK_IDENTITIES: &str = "sessions/agent-session-identities.json";
const DESK_IDENTITIES_BACKUP: &str = "sessions/agent-session-identities.json.bak";

pub(super) fn select_desk(
    capture: &SourceCapture,
    decoder: &dyn LegacyDecoder,
    selection: &SourceSelection,
    statuses: &mut Statuses,
) -> Result<
    (
        Vec<(ImportDomain, ImportProvenance)>,
        DomainImport,
        Vec<IgnoredKeys>,
    ),
    ImportRefusal,
> {
    let (snapshot_backup, identity_backup, acknowledge_absent) = match selection {
        SourceSelection::Current => (None, None, false),
        SourceSelection::Backup(relative) => match capture
            .files()
            .iter()
            .find(|file| file.domain == ImportDomain::Desk && file.relative == *relative)
        {
            Some(file)
                if file.role == SourceRole::RotatingBackup
                    && file.relative == DESK_IDENTITIES_BACKUP =>
            {
                (None, Some(relative.clone()), false)
            }
            Some(file)
                if file.role == SourceRole::RotatingBackup
                    && file.relative.starts_with("sessions/backups/") =>
            {
                (Some(relative.clone()), None, false)
            }
            Some(file) if file.role == SourceRole::NamedSnapshot => {
                return Err(ImportRefusal::InvalidSelection {
                    domain: ImportDomain::Desk,
                    reason: "named snapshots are archive-only; select a rotating snapshot backup",
                })
            }
            _ => {
                return Err(ImportRefusal::InvalidSelection {
                    domain: ImportDomain::Desk,
                    reason: "names no desk snapshot or identity backup",
                })
            }
        },
        SourceSelection::DeskBackups {
            snapshot,
            identities,
        } => (snapshot.clone(), identities.clone(), false),
        SourceSelection::AcknowledgeAbsent => (None, None, true),
    };
    let snapshot_current = source(capture, DESK_SNAPSHOT);
    let identity_current = source(capture, DESK_IDENTITIES);
    let identity_recovery = source(capture, DESK_IDENTITIES_BACKUP);
    let snapshot_files_exist = capture.files().iter().any(|file| {
        file.domain == ImportDomain::Desk
            && file.role == SourceRole::RotatingBackup
            && file.relative.starts_with("sessions/backups/")
    });
    let has_desk_files = capture
        .files()
        .iter()
        .any(|file| file.domain == ImportDomain::Desk);
    if !has_desk_files {
        if snapshot_backup.is_some() || identity_backup.is_some() {
            return Err(ImportRefusal::InvalidSelection {
                domain: ImportDomain::Desk,
                reason: "no desk recovery source exists",
            });
        }
        return Ok((
            vec![(ImportDomain::Desk, ImportProvenance::FirstInstall)],
            DomainImport::Desk(empty()),
            Vec::new(),
        ));
    }

    if acknowledge_absent && snapshot_current.is_some() {
        return Err(ImportRefusal::InvalidSelection {
            domain: ImportDomain::Desk,
            reason: "the current desk snapshot exists",
        });
    }

    let (mut snapshot, snapshot_origin) = if let Some(relative) = snapshot_backup {
        let backup = capture.files().iter().find(|file| {
            file.domain == ImportDomain::Desk
                && file.relative == relative
                && file.role == SourceRole::RotatingBackup
                && file.relative.starts_with("sessions/backups/")
        });
        let Some(backup) = backup else {
            let named = capture.files().iter().any(|file| {
                file.domain == ImportDomain::Desk
                    && file.relative == relative
                    && file.role == SourceRole::NamedSnapshot
            });
            return Err(ImportRefusal::InvalidSelection {
                domain: ImportDomain::Desk,
                reason: if named {
                    "named snapshots are archive-only; select a rotating snapshot backup"
                } else {
                    "select a rotating snapshot backup from sessions/backups"
                },
            });
        };
        if let Some(current) = snapshot_current {
            match decode_desk_snapshot(capture, decoder, current, statuses)? {
                Ok(_) => {
                    return Err(ImportRefusal::InvalidSelection {
                        domain: ImportDomain::Desk,
                        reason: "a snapshot backup stands in only for an absent or damaged current snapshot",
                    })
                }
                Err(status) if status.recoverable() => {}
                Err(_) => {
                    return Err(ImportRefusal::InvalidSelection {
                        domain: ImportDomain::Desk,
                        reason: "a snapshot backup cannot replace an unsupported or conflicting current snapshot",
                    })
                }
            }
        }
        let snapshot =
            decode_desk_snapshot(capture, decoder, backup, statuses)?.map_err(|status| {
                ImportRefusal::Decode {
                    domain: ImportDomain::Desk,
                    relative: backup.relative.clone(),
                    status,
                }
            })?;
        (snapshot, ImportProvenance::Backup(backup.relative.clone()))
    } else if acknowledge_absent {
        (empty(), ImportProvenance::AcknowledgedAbsent)
    } else if let Some(current) = snapshot_current {
        match decode_desk_snapshot(capture, decoder, current, statuses)? {
            Ok(snapshot) => (
                snapshot,
                ImportProvenance::Current(current.relative.clone()),
            ),
            Err(status) if status.recoverable() => {
                return Err(ImportRefusal::RecoveryRequired {
                    domain: ImportDomain::Desk,
                    current: status,
                    candidates: desk_snapshot_candidates(capture, decoder, statuses)?,
                })
            }
            Err(status) => {
                return Err(ImportRefusal::Decode {
                    domain: ImportDomain::Desk,
                    relative: current.relative.clone(),
                    status,
                })
            }
        }
    } else if snapshot_files_exist {
        return Err(ImportRefusal::RecoveryRequired {
            domain: ImportDomain::Desk,
            current: SourceStatus::Absent,
            candidates: desk_snapshot_candidates(capture, decoder, statuses)?,
        });
    } else if matches!(selection, SourceSelection::DeskBackups { .. }) {
        (empty(), ImportProvenance::AcknowledgedAbsent)
    } else {
        return Err(ImportRefusal::RecoveryRequired {
            domain: ImportDomain::Desk,
            current: SourceStatus::Absent,
            candidates: Vec::new(),
        });
    };

    let (identities, identity_origin) = if let Some(relative) = identity_backup {
        let backup = capture
            .files()
            .iter()
            .find(|file| {
                file.domain == ImportDomain::Desk
                    && file.relative == relative
                    && file.role == SourceRole::RotatingBackup
                    && file.relative == DESK_IDENTITIES_BACKUP
            })
            .ok_or(ImportRefusal::InvalidSelection {
                domain: ImportDomain::Desk,
                reason: "names no desk identity-catalogue backup",
            })?;
        if let Some(current) = identity_current {
            match decode_desk_identities(capture, decoder, current, statuses)? {
                Ok(_) => {
                    return Err(ImportRefusal::InvalidSelection {
                        domain: ImportDomain::Desk,
                        reason:
                            "an identity backup stands in only for an absent or damaged catalogue",
                    })
                }
                Err(status) if status.recoverable() => {}
                Err(_) => return Err(ImportRefusal::InvalidSelection {
                    domain: ImportDomain::Desk,
                    reason:
                        "an identity backup cannot replace an unsupported or conflicting catalogue",
                }),
            }
        }
        let identities =
            decode_desk_identities(capture, decoder, backup, statuses)?.map_err(|status| {
                ImportRefusal::Decode {
                    domain: ImportDomain::Desk,
                    relative: backup.relative.clone(),
                    status,
                }
            })?;
        (
            identities,
            ImportProvenance::Backup(backup.relative.clone()),
        )
    } else if let Some(current) = identity_current {
        match decode_desk_identities(capture, decoder, current, statuses)? {
            Ok(identities) => (
                identities,
                ImportProvenance::Current(current.relative.clone()),
            ),
            Err(status) if status.recoverable() => {
                return Err(ImportRefusal::RecoveryRequired {
                    domain: ImportDomain::Desk,
                    current: status,
                    candidates: identity_recovery_candidates(
                        capture,
                        decoder,
                        identity_recovery,
                        statuses,
                    )?,
                })
            }
            Err(status) => {
                return Err(ImportRefusal::Decode {
                    domain: ImportDomain::Desk,
                    relative: current.relative.clone(),
                    status,
                })
            }
        }
    } else if identity_recovery.is_some() {
        return Err(ImportRefusal::RecoveryRequired {
            domain: ImportDomain::Desk,
            current: SourceStatus::Absent,
            candidates: identity_recovery_candidates(
                capture,
                decoder,
                identity_recovery,
                statuses,
            )?,
        });
    } else {
        (Vec::new(), ImportProvenance::FirstInstall)
    };
    snapshot.agent_session_identities = identities;
    let provenance = vec![
        (ImportDomain::Desk, snapshot_origin),
        (ImportDomain::Desk, identity_origin),
    ];
    Ok((
        provenance,
        DomainImport::Desk(normalize(&snapshot)),
        Vec::new(),
    ))
}

fn source<'a>(capture: &'a SourceCapture, relative: &str) -> Option<&'a SourceFile> {
    capture
        .files()
        .iter()
        .find(|file| file.domain == ImportDomain::Desk && file.relative == relative)
}

fn decode_desk_snapshot(
    capture: &SourceCapture,
    decoder: &dyn LegacyDecoder,
    file: &SourceFile,
    statuses: &mut Statuses,
) -> Result<Result<crate::repositories::desk::DeskSnapshot, SourceStatus>, ImportRefusal> {
    let decoded = match &file.captured {
        Captured::Unreadable { kind, detail } => Err(SourceStatus::unreadable(*kind, detail)),
        Captured::Bytes { .. } => decoder
            .decode_desk_snapshot(&capture.read(file)?)
            .map_err(SourceStatus::from),
    };
    statuses.push((
        ImportDomain::Desk,
        file.relative.clone(),
        decoded
            .as_ref()
            .err()
            .cloned()
            .unwrap_or(SourceStatus::Valid),
    ));
    Ok(decoded)
}

fn decode_desk_identities(
    capture: &SourceCapture,
    decoder: &dyn LegacyDecoder,
    file: &SourceFile,
    statuses: &mut Statuses,
) -> Result<
    Result<Vec<crate::repositories::desk::DeskAgentSessionIdentity>, SourceStatus>,
    ImportRefusal,
> {
    let decoded = match &file.captured {
        Captured::Unreadable { kind, detail } => Err(SourceStatus::unreadable(*kind, detail)),
        Captured::Bytes { .. } => decoder
            .decode_desk_identities(&capture.read(file)?)
            .map_err(SourceStatus::from),
    };
    statuses.push((
        ImportDomain::Desk,
        file.relative.clone(),
        decoded
            .as_ref()
            .err()
            .cloned()
            .unwrap_or(SourceStatus::Valid),
    ));
    Ok(decoded)
}

fn desk_snapshot_candidates(
    capture: &SourceCapture,
    decoder: &dyn LegacyDecoder,
    statuses: &mut Statuses,
) -> Result<Vec<RecoveryCandidate>, ImportRefusal> {
    let mut found = Vec::new();
    for file in capture.files().iter().filter(|file| {
        file.domain == ImportDomain::Desk
            && file.role == SourceRole::RotatingBackup
            && file.relative.starts_with("sessions/backups/")
    }) {
        let status = match decode_desk_snapshot(capture, decoder, file, statuses)? {
            Ok(_) => SourceStatus::Valid,
            Err(status) => status,
        };
        found.push(RecoveryCandidate {
            relative: file.relative.clone(),
            role: file.role,
            status,
        });
    }
    Ok(found)
}

fn identity_recovery_candidates(
    capture: &SourceCapture,
    decoder: &dyn LegacyDecoder,
    file: Option<&SourceFile>,
    statuses: &mut Statuses,
) -> Result<Vec<RecoveryCandidate>, ImportRefusal> {
    let Some(file) = file else {
        return Ok(Vec::new());
    };
    let status = match decode_desk_identities(capture, decoder, file, statuses)? {
        Ok(_) => SourceStatus::Valid,
        Err(status) => status,
    };
    Ok(vec![RecoveryCandidate {
        relative: file.relative.clone(),
        role: file.role,
        status,
    }])
}
