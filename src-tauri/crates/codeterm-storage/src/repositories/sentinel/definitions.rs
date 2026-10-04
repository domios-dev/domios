//! Immutable definition versions, their ordered buckets/handlers/targets and the mutable head.

use super::records::{
    BucketDefinition, CommandParameters, DefinitionProvenance, HandlerAction, HandlerDefinition,
    StageDefinition, StoredDefinition, TargetIdentity,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::{from_sql_u64, to_sql_u64, Revision};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

fn corrupt(detail: &str) -> StorageError {
    StorageError::Corrupt {
        detail: format!("sentinel definition: {detail}"),
    }
}

fn provenance_text(provenance: DefinitionProvenance) -> &'static str {
    match provenance {
        DefinitionProvenance::Authored => "authored",
        DefinitionProvenance::Imported => "imported",
    }
}

fn provenance_from(text: &str) -> Result<DefinitionProvenance, StorageError> {
    match text {
        "authored" => Ok(DefinitionProvenance::Authored),
        "imported" => Ok(DefinitionProvenance::Imported),
        _ => Err(corrupt("unknown provenance")),
    }
}

/// (kind, target_ref, command, label, timeout_ms) columns of one handler action.
type ActionColumns<'a> = (
    &'static str,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<i64>,
);

fn action_columns(action: &HandlerAction) -> Result<ActionColumns<'_>, StorageError> {
    Ok(match action {
        HandlerAction::Notify => ("notify", None, None, None, None),
        HandlerAction::Stop => ("stop", None, None, None, None),
        HandlerAction::Send(target) => ("send", Some(target.as_str()), None, None, None),
        HandlerAction::Cron(target) => ("cron", Some(target.as_str()), None, None, None),
        HandlerAction::Sentinel(target) => ("sentinel", Some(target.as_str()), None, None, None),
        HandlerAction::Command(parameters) => (
            "command",
            None,
            Some(parameters.command.as_str()),
            Some(parameters.label.as_str()),
            Some(to_sql_u64("command timeout", parameters.timeout_ms)?),
        ),
    })
}

fn action_from(
    kind: &str,
    target: Option<String>,
    command: Option<String>,
    label: Option<String>,
    timeout_ms: Option<i64>,
) -> Result<HandlerAction, StorageError> {
    let action = match (kind, target, command, label, timeout_ms) {
        ("notify", None, None, None, None) => HandlerAction::Notify,
        ("stop", None, None, None, None) => HandlerAction::Stop,
        ("send", Some(target), None, None, None) => HandlerAction::Send(target),
        ("cron", Some(target), None, None, None) => HandlerAction::Cron(target),
        ("sentinel", Some(target), None, None, None) => HandlerAction::Sentinel(target),
        ("command", None, Some(command), Some(label), Some(timeout_ms)) => {
            HandlerAction::Command(CommandParameters {
                command,
                label,
                timeout_ms: from_sql_u64("command timeout", timeout_ms)?,
            })
        }
        _ => return Err(corrupt("handler action columns disagree")),
    };
    Ok(action)
}

/// Writes one immutable version; callers move the head separately.
pub(super) fn insert_version(
    cx: &mut WriteCx<'_>,
    id: &str,
    version: u64,
    provenance: DefinitionProvenance,
    definition: &StageDefinition,
) -> Result<(), StorageError> {
    let version = to_sql_u64("definition version", version)?;
    let created = to_sql_u64("revision", cx.revision.0)?;
    cx.write(
        "INSERT INTO sentinel_definitions (sentinel_id, version, provenance, name, probe, \
         question, interval_ms, until_bucket, max_transitions, max_actions, created_revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            version,
            provenance_text(provenance),
            definition.name,
            definition.probe,
            definition.question,
            to_sql_u64("sentinel interval", definition.interval_ms)?,
            definition.until,
            i64::from(definition.max_transitions),
            i64::from(definition.max_actions),
            created
        ],
    )?;
    for (ordinal, bucket) in (0_i64..).zip(&definition.buckets) {
        cx.write(
            "INSERT INTO sentinel_buckets (sentinel_id, version, ordinal, bucket, match_pattern) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, version, ordinal, bucket.name, bucket.match_pattern],
        )?;
        for (position, handler) in (0_i64..).zip(&bucket.handlers) {
            let (kind, target, command, label, timeout) = action_columns(&handler.action)?;
            cx.write(
                "INSERT INTO sentinel_handlers (sentinel_id, version, bucket, ordinal, \
                 handler_id, kind, target_ref, command, label, timeout_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    version,
                    bucket.name,
                    position,
                    handler.id,
                    kind,
                    target,
                    command,
                    label,
                    timeout
                ],
            )?;
            if let Some(exact) = &handler.target {
                cx.write(
                    "INSERT INTO sentinel_handler_targets (sentinel_id, version, handler_id, \
                     kind, target_id, workspace_id, team_id, owner_tab_id) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        id,
                        version,
                        handler.id,
                        exact.kind,
                        exact.id,
                        exact.workspace_id,
                        exact.team_id,
                        exact.owner_tab_id
                    ],
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn insert_head(
    cx: &mut WriteCx<'_>,
    id: &str,
    version: u64,
) -> Result<(), StorageError> {
    cx.write(
        "INSERT INTO sentinel_heads (sentinel_id, version) VALUES (?1, ?2)",
        params![id, to_sql_u64("definition version", version)?],
    )?;
    Ok(())
}

pub(super) fn move_head(cx: &mut WriteCx<'_>, id: &str, version: u64) -> Result<(), StorageError> {
    cx.write(
        "UPDATE sentinel_heads SET version = ?2 WHERE sentinel_id = ?1",
        params![id, to_sql_u64("definition version", version)?],
    )?;
    Ok(())
}

pub(super) fn head_version(conn: &Connection, id: &str) -> Result<Option<u64>, StorageError> {
    let stored: Option<i64> = conn
        .prepare_cached("SELECT version FROM sentinel_heads WHERE sentinel_id = ?1")
        .and_then(|mut statement| statement.query_row([id], |row| row.get(0)).optional())
        .map_err(|e| sqlite_error("read sentinel head", e))?;
    stored
        .map(|version| from_sql_u64("definition version", version))
        .transpose()
}

pub(super) fn latest_version(conn: &Connection, id: &str) -> Result<u64, StorageError> {
    let latest: i64 = conn
        .prepare_cached(
            "SELECT COALESCE(MAX(version), 0) FROM sentinel_definitions WHERE sentinel_id = ?1",
        )
        .and_then(|mut statement| statement.query_row([id], |row| row.get(0)))
        .map_err(|e| sqlite_error("read sentinel versions", e))?;
    from_sql_u64("definition version", latest)
}

/// `(max_transitions, max_actions)` of the current head version.
pub(super) fn head_budgets(conn: &Connection, id: &str) -> Result<(u64, u64), StorageError> {
    conn.prepare_cached(
        "SELECT d.max_transitions, d.max_actions FROM sentinel_heads h \
         JOIN sentinel_definitions d ON d.sentinel_id = h.sentinel_id AND d.version = h.version \
         WHERE h.sentinel_id = ?1",
    )
    .and_then(|mut statement| {
        statement.query_row([id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
    })
    .map_err(|e| sqlite_error("read sentinel budgets", e))
    .and_then(|(transitions, actions)| {
        Ok((
            from_sql_u64("max transitions", transitions)?,
            from_sql_u64("max actions", actions)?,
        ))
    })
}

/// Whether the head version declares `handler` in `bucket`, and the handler's action.
pub(super) fn head_handler(
    conn: &Connection,
    id: &str,
    bucket: &str,
    handler: &str,
) -> Result<Option<HandlerAction>, StorageError> {
    let row = conn
        .prepare_cached(
            "SELECT hd.kind, hd.target_ref, hd.command, hd.label, hd.timeout_ms \
             FROM sentinel_heads h JOIN sentinel_handlers hd \
             ON hd.sentinel_id = h.sentinel_id AND hd.version = h.version \
             WHERE h.sentinel_id = ?1 AND hd.bucket = ?2 AND hd.handler_id = ?3",
        )
        .and_then(|mut statement| {
            statement
                .query_row(params![id, bucket, handler], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                })
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel handler", e))?;
    row.map(|(kind, target, command, label, timeout)| {
        action_from(&kind, target, command, label, timeout)
    })
    .transpose()
}

pub(super) fn load_version(
    conn: &Connection,
    id: &str,
    version: u64,
) -> Result<StoredDefinition, StorageError> {
    let sql_version = to_sql_u64("definition version", version)?;
    let header = conn
        .prepare_cached(
            "SELECT provenance, name, probe, question, interval_ms, until_bucket, \
             max_transitions, max_actions, created_revision FROM sentinel_definitions \
             WHERE sentinel_id = ?1 AND version = ?2",
        )
        .and_then(|mut statement| {
            statement
                .query_row(params![id, sql_version], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                    ))
                })
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel definition", e))?
        .ok_or_else(|| corrupt("head names a missing version"))?;
    let (provenance, name, probe, question, interval, until, transitions, actions, created) =
        header;
    let budget = |field: &'static str, value: i64| u16::try_from(value).map_err(|_| corrupt(field));
    Ok(StoredDefinition {
        version,
        provenance: provenance_from(&provenance)?,
        created_revision: Revision(from_sql_u64("revision", created)?),
        definition: StageDefinition {
            name,
            probe,
            question,
            interval_ms: from_sql_u64("sentinel interval", interval)?,
            until,
            max_transitions: budget("max transitions", transitions)?,
            max_actions: budget("max actions", actions)?,
            buckets: load_buckets(conn, id, sql_version)?,
        },
    })
}

fn load_buckets(
    conn: &Connection,
    id: &str,
    version: i64,
) -> Result<Vec<BucketDefinition>, StorageError> {
    let mut buckets: Vec<BucketDefinition> = {
        let mut statement = conn
            .prepare_cached(
                "SELECT bucket, match_pattern FROM sentinel_buckets \
                 WHERE sentinel_id = ?1 AND version = ?2 ORDER BY ordinal",
            )
            .map_err(|e| sqlite_error("read sentinel buckets", e))?;
        let rows = statement
            .query_map(params![id, version], |row| {
                Ok(BucketDefinition {
                    name: row.get(0)?,
                    match_pattern: row.get(1)?,
                    handlers: Vec::new(),
                })
            })
            .map_err(|e| sqlite_error("read sentinel buckets", e))?;
        rows.collect::<Result<_, _>>()
            .map_err(|e| sqlite_error("read sentinel buckets", e))?
    };
    let mut targets = load_targets(conn, id, version)?;
    let index: HashMap<String, usize> = buckets
        .iter()
        .enumerate()
        .map(|(position, bucket)| (bucket.name.clone(), position))
        .collect();
    let mut statement = conn
        .prepare_cached(
            "SELECT bucket, handler_id, kind, target_ref, command, label, timeout_ms \
             FROM sentinel_handlers WHERE sentinel_id = ?1 AND version = ?2 \
             ORDER BY bucket, ordinal",
        )
        .map_err(|e| sqlite_error("read sentinel handlers", e))?;
    let rows = statement
        .query_map(params![id, version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        })
        .map_err(|e| sqlite_error("read sentinel handlers", e))?;
    for row in rows {
        let (bucket, handler, kind, target, command, label, timeout) =
            row.map_err(|e| sqlite_error("read sentinel handlers", e))?;
        let position = *index
            .get(&bucket)
            .ok_or_else(|| corrupt("handler names an undeclared bucket"))?;
        buckets[position].handlers.push(HandlerDefinition {
            target: targets.remove(&handler),
            action: action_from(&kind, target, command, label, timeout)?,
            id: handler,
        });
    }
    if !targets.is_empty() {
        return Err(corrupt("target names a missing handler"));
    }
    Ok(buckets)
}

fn load_targets(
    conn: &Connection,
    id: &str,
    version: i64,
) -> Result<HashMap<String, TargetIdentity>, StorageError> {
    let mut statement = conn
        .prepare_cached(
            "SELECT handler_id, kind, target_id, workspace_id, team_id, owner_tab_id \
             FROM sentinel_handler_targets WHERE sentinel_id = ?1 AND version = ?2",
        )
        .map_err(|e| sqlite_error("read sentinel targets", e))?;
    let rows = statement
        .query_map(params![id, version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                TargetIdentity {
                    kind: row.get(1)?,
                    id: row.get(2)?,
                    workspace_id: row.get(3)?,
                    team_id: row.get(4)?,
                    owner_tab_id: row.get(5)?,
                },
            ))
        })
        .map_err(|e| sqlite_error("read sentinel targets", e))?;
    rows.collect::<Result<_, _>>()
        .map_err(|e| sqlite_error("read sentinel targets", e))
}

pub(super) fn load_history(
    conn: &Connection,
    id: &str,
) -> Result<Vec<StoredDefinition>, StorageError> {
    let versions: Vec<i64> = {
        let mut statement = conn
            .prepare_cached(
                "SELECT version FROM sentinel_definitions WHERE sentinel_id = ?1 ORDER BY version",
            )
            .map_err(|e| sqlite_error("read sentinel versions", e))?;
        let rows = statement
            .query_map([id], |row| row.get(0))
            .map_err(|e| sqlite_error("read sentinel versions", e))?;
        rows.collect::<Result<_, _>>()
            .map_err(|e| sqlite_error("read sentinel versions", e))?
    };
    versions
        .into_iter()
        .map(|version| load_version(conn, id, from_sql_u64("definition version", version)?))
        .collect()
}
