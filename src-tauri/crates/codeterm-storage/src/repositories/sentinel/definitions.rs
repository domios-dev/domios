//! Immutable definition versions, their ordered buckets/handlers/targets and the mutable head.

use super::records::{
    AmendmentRecord, BucketDefinition, CommandParameters, DefinitionProvenance, EscalateTarget,
    HandlerAction, HandlerDefinition, ObserveKind, ObserveSource, SafeKind, SourceKind,
    StageDefinition, StageSource, StoredDefinition, TargetIdentity, MAX_SENTINEL_LINEAGE,
    OTHER_BUCKET,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::{from_sql_u64, to_sql_u64, Revision};
use codeterm_blueprint_types::sentinel_input::InputSpec;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::collections::HashMap;

fn corrupt(detail: &str) -> StorageError {
    StorageError::Corrupt {
        detail: format!("blueprint definition: {detail}"),
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

fn observe_kind_text(kind: ObserveKind) -> &'static str {
    match kind {
        ObserveKind::Tab => "tab",
        ObserveKind::CronRun => "cron_run",
    }
}

fn observe_from(
    kind: Option<String>,
    reference: Option<String>,
) -> Result<Option<ObserveSource>, StorageError> {
    let kind = match (kind.as_deref(), &reference) {
        (None, None) => return Ok(None),
        (Some("tab"), Some(_)) => ObserveKind::Tab,
        (Some("cron_run"), Some(_)) => ObserveKind::CronRun,
        _ => return Err(corrupt("observe source columns disagree")),
    };
    Ok(reference.map(|reference| ObserveSource { kind, reference }))
}

/// The `source_kind`/`source_spec`/`probe`/`observe_*` column group of one definition row.
struct SourceColumns {
    kind: SourceKind,
    spec: Option<String>,
    probe: String,
    observe_kind: Option<&'static str>,
    observe_ref: Option<String>,
}

fn source_columns(source: &StageSource) -> Result<SourceColumns, StorageError> {
    Ok(match source {
        StageSource::Probe(command) => SourceColumns {
            kind: SourceKind::Probe,
            spec: None,
            probe: command.clone(),
            observe_kind: None,
            observe_ref: None,
        },
        StageSource::Observe(observe) => SourceColumns {
            kind: SourceKind::Observe,
            spec: None,
            probe: String::new(),
            observe_kind: Some(observe_kind_text(observe.kind)),
            observe_ref: Some(observe.reference.clone()),
        },
        StageSource::Terminal { binding, .. } => SourceColumns {
            kind: SourceKind::Terminal,
            spec: None,
            probe: String::new(),
            observe_kind: Some(observe_kind_text(binding.kind)),
            observe_ref: Some(binding.reference.clone()),
        },
        StageSource::Pipeline(_) => SourceColumns {
            kind: SourceKind::Pipeline,
            spec: None,
            probe: String::new(),
            observe_kind: None,
            observe_ref: None,
        },
        StageSource::Input(input) => SourceColumns {
            kind: SourceKind::Input,
            spec: Some(serde_json::to_string(input).map_err(|_| {
                StorageError::invalid("blueprint input", "cannot be encoded for storage")
            })?),
            probe: String::new(),
            observe_kind: None,
            observe_ref: None,
        },
    })
}

fn source_from(
    kind: &str,
    spec: Option<String>,
    probe: String,
    observe: Option<ObserveSource>,
    terminal: Option<codeterm_blueprint_types::blueprint_terminal::Config>,
    pipeline: Option<codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec>,
) -> Result<StageSource, StorageError> {
    match SourceKind::parse(kind) {
        Some(SourceKind::Probe) if !probe.is_empty() && observe.is_none() && spec.is_none() => {
            Ok(StageSource::Probe(probe))
        }
        Some(SourceKind::Observe) if probe.is_empty() && spec.is_none() => observe
            .map(StageSource::Observe)
            .ok_or_else(|| corrupt("observe source columns disagree")),
        Some(SourceKind::Input) if probe.is_empty() && observe.is_none() => {
            let spec = spec.ok_or_else(|| corrupt("input source lacks its spec"))?;
            let input: InputSpec = serde_json::from_str(&spec)
                .map_err(|_| corrupt("input source spec does not decode"))?;
            Ok(StageSource::Input(input))
        }
        Some(SourceKind::Terminal) if probe.is_empty() && spec.is_none() => {
            let binding = observe.ok_or_else(|| corrupt("terminal source lacks its binding"))?;
            let config = terminal.ok_or_else(|| corrupt("terminal source lacks its config"))?;
            Ok(StageSource::Terminal { binding, config })
        }
        Some(SourceKind::Pipeline) if probe.is_empty() && observe.is_none() && spec.is_none() => {
            pipeline
                .map(StageSource::Pipeline)
                .ok_or_else(|| corrupt("pipeline source lacks its spec"))
        }
        Some(_) => Err(corrupt("stage source columns disagree")),
        None if kind == "event" => Err(corrupt("event source is not readable in this build")),
        None => Err(corrupt("unknown source kind")),
    }
}

fn safe_kind_text(kind: &SafeKind) -> &str {
    match kind {
        SafeKind::DirectoryTrust => "directory_trust",
        SafeKind::Onboarding => "onboarding",
        SafeKind::Update => "update",
        SafeKind::Permission => "permission",
        SafeKind::Structured => "structured",
        SafeKind::Other(kind) => kind,
    }
}

fn safe_kind_from(text: &str) -> Result<SafeKind, StorageError> {
    Ok(match text {
        "directory_trust" => SafeKind::DirectoryTrust,
        "onboarding" => SafeKind::Onboarding,
        "update" => SafeKind::Update,
        "permission" => SafeKind::Permission,
        "structured" => SafeKind::Structured,
        other => SafeKind::Other(other.to_string()),
    })
}

/// The handler action columns of one `sentinel_handlers` row.
struct ActionColumns<'a> {
    kind: &'static str,
    target: Option<&'a str>,
    command: Option<&'a str>,
    label: Option<&'a str>,
    timeout_ms: Option<i64>,
    safe_kind: Option<&'a str>,
    escalate_to: Option<&'static str>,
}

impl<'a> ActionColumns<'a> {
    fn plain(kind: &'static str, target: Option<&'a str>) -> Self {
        ActionColumns {
            kind,
            target,
            command: None,
            label: None,
            timeout_ms: None,
            safe_kind: None,
            escalate_to: None,
        }
    }
}

fn action_columns(action: &HandlerAction) -> Result<ActionColumns<'_>, StorageError> {
    Ok(match action {
        HandlerAction::Notify => ActionColumns::plain("notify", None),
        HandlerAction::Stop => ActionColumns::plain("stop", None),
        HandlerAction::Send(target) => ActionColumns::plain("send", Some(target.as_str())),
        HandlerAction::Cron(target) => ActionColumns::plain("cron", Some(target.as_str())),
        HandlerAction::Sentinel(target) => ActionColumns::plain("blueprint", Some(target.as_str())),
        HandlerAction::Command(parameters) => ActionColumns {
            command: Some(parameters.command.as_str()),
            label: Some(parameters.label.as_str()),
            timeout_ms: Some(to_sql_u64("command timeout", parameters.timeout_ms)?),
            ..ActionColumns::plain("command", None)
        },
        HandlerAction::RespondPrompt(kind) => ActionColumns {
            safe_kind: Some(safe_kind_text(kind)),
            ..ActionColumns::plain("respond_prompt", None)
        },
        HandlerAction::Escalate(EscalateTarget::Creator) => ActionColumns {
            escalate_to: Some("creator"),
            ..ActionColumns::plain("escalate", None)
        },
        HandlerAction::Escalate(EscalateTarget::Agent(tab)) => ActionColumns {
            escalate_to: Some("agent"),
            ..ActionColumns::plain("escalate", Some(tab.as_str()))
        },
    })
}

/// `kind, target_ref, command, label, timeout_ms, safe_kind, escalate_to`, read from `start`.
struct ActionRow {
    kind: String,
    target: Option<String>,
    command: Option<String>,
    label: Option<String>,
    timeout_ms: Option<i64>,
    safe_kind: Option<String>,
    escalate_to: Option<String>,
}

impl ActionRow {
    fn read(row: &Row<'_>, start: usize) -> rusqlite::Result<Self> {
        Ok(ActionRow {
            kind: row.get(start)?,
            target: row.get(start + 1)?,
            command: row.get(start + 2)?,
            label: row.get(start + 3)?,
            timeout_ms: row.get(start + 4)?,
            safe_kind: row.get(start + 5)?,
            escalate_to: row.get(start + 6)?,
        })
    }

    fn action(self) -> Result<HandlerAction, StorageError> {
        let ActionRow {
            kind,
            target,
            command,
            label,
            timeout_ms,
            safe_kind,
            escalate_to,
        } = self;
        if kind != "command" && (command.is_some() || label.is_some() || timeout_ms.is_some()) {
            return Err(corrupt("handler action columns disagree"));
        }
        let action = match (kind.as_str(), target, safe_kind, escalate_to.as_deref()) {
            ("notify", None, None, None) => HandlerAction::Notify,
            ("stop", None, None, None) => HandlerAction::Stop,
            ("send", Some(target), None, None) => HandlerAction::Send(target),
            ("cron", Some(target), None, None) => HandlerAction::Cron(target),
            ("blueprint", Some(target), None, None) => HandlerAction::Sentinel(target),
            ("respond_prompt", None, Some(safe_kind), None) => {
                HandlerAction::RespondPrompt(safe_kind_from(&safe_kind)?)
            }
            ("escalate", None, None, Some("creator")) => {
                HandlerAction::Escalate(EscalateTarget::Creator)
            }
            ("escalate", Some(tab), None, Some("agent")) => {
                HandlerAction::Escalate(EscalateTarget::Agent(tab))
            }
            ("command", None, None, None) => match (command, label, timeout_ms) {
                (Some(command), Some(label), Some(timeout_ms)) => {
                    HandlerAction::Command(CommandParameters {
                        command,
                        label,
                        timeout_ms: from_sql_u64("command timeout", timeout_ms)?,
                    })
                }
                _ => return Err(corrupt("handler action columns disagree")),
            },
            _ => return Err(corrupt("handler action columns disagree")),
        };
        Ok(action)
    }
}

/// Lineage columns are all present on an amended version and all absent otherwise.
fn amendment_from(
    version: u64,
    replaced: Option<i64>,
    author: Option<String>,
    cause: Option<String>,
    authored_at: Option<i64>,
) -> Result<Option<AmendmentRecord>, StorageError> {
    match (replaced, author, authored_at) {
        (None, None, None) if cause.is_none() => Ok(None),
        (Some(replaced), Some(author_tab_id), Some(authored_at)) => Ok(Some(AmendmentRecord {
            version,
            replaced_version: from_sql_u64("replaced version", replaced)?,
            author_tab_id,
            cause_transition_id: cause,
            authored_at: from_sql_u64("amendment time", authored_at)?,
        })),
        _ => Err(corrupt("amendment lineage columns disagree")),
    }
}

/// Writes one immutable version; callers move the head separately.
pub(super) fn insert_version(
    cx: &mut WriteCx<'_>,
    id: &str,
    version: u64,
    provenance: DefinitionProvenance,
    amendment: Option<&AmendmentRecord>,
    definition: &StageDefinition,
) -> Result<(), StorageError> {
    if amendment.is_some_and(|amendment| amendment.version != version) {
        return Err(StorageError::invalid(
            "amendment",
            "must describe the version it is stored with",
        ));
    }
    let sql_version = to_sql_u64("definition version", version)?;
    let created = to_sql_u64("revision", cx.revision.0)?;
    let source = source_columns(&definition.source)?;
    let pipeline_spec = definition
        .pipeline()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| StorageError::invalid("pipeline", "cannot be encoded for storage"))?;
    let escalation = definition
        .escalation_timeout_ms
        .map(|timeout| to_sql_u64("escalation timeout", timeout))
        .transpose()?;
    let replaced = amendment
        .map(|amendment| to_sql_u64("replaced version", amendment.replaced_version))
        .transpose()?;
    let authored_at = amendment
        .map(|amendment| to_sql_u64("amendment time", amendment.authored_at))
        .transpose()?;
    cx.write(
        "INSERT INTO blueprint_definitions (blueprint_id, version, provenance, name, source_kind, \
         source_spec, pipeline_spec, probe, observe_kind, observe_ref, question, interval_ms, until_bucket, \
         escalation_timeout_ms, max_transitions, max_actions, replaced_version, author_tab_id, \
         cause_transition_id, authored_at, created_revision, terminal_spec, session, model_classification) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
         ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
        params![
            id,
            sql_version,
            provenance_text(provenance),
            definition.name,
            source.kind.as_str(),
            source.spec,
            pipeline_spec,
            source.probe,
            source.observe_kind,
            source.observe_ref,
            definition.question,
            to_sql_u64("sentinel interval", definition.interval_ms)?,
            definition.until,
            escalation,
            i64::from(definition.max_transitions),
            i64::from(definition.max_actions),
            replaced,
            amendment.map(|amendment| amendment.author_tab_id.as_str()),
            amendment.and_then(|amendment| amendment.cause_transition_id.as_deref()),
            authored_at,
            created,
            definition.terminal().map(serde_json::to_string).transpose().map_err(|_| StorageError::invalid("terminal", "cannot be encoded"))?,
            definition.session,
            definition.model_classification
        ],
    )?;
    for (ordinal, bucket) in (0_i64..).zip(&definition.buckets) {
        insert_bucket(
            cx,
            id,
            sql_version,
            ordinal,
            &bucket.name,
            bucket.match_pattern.as_deref(),
            bucket.rearm_on_change,
            &bucket.handlers,
        )?;
    }
    if !definition.on_other.is_empty() {
        let ordinal = i64::try_from(definition.buckets.len())
            .map_err(|_| StorageError::invalid("sentinel buckets", "too many buckets"))?;
        insert_bucket(
            cx,
            id,
            sql_version,
            ordinal,
            OTHER_BUCKET,
            None,
            false,
            &definition.on_other,
        )?;
    }
    Ok(())
}

fn insert_bucket(
    cx: &mut WriteCx<'_>,
    id: &str,
    version: i64,
    ordinal: i64,
    bucket: &str,
    match_pattern: Option<&str>,
    rearm_on_change: bool,
    handlers: &[HandlerDefinition],
) -> Result<(), StorageError> {
    cx.write(
        "INSERT INTO blueprint_buckets (blueprint_id, version, ordinal, bucket, match_pattern, rearm_on_change) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, version, ordinal, bucket, match_pattern, rearm_on_change],
    )?;
    for (position, handler) in (0_i64..).zip(handlers) {
        let columns = action_columns(&handler.action)?;
        cx.write(
            "INSERT INTO blueprint_handlers (blueprint_id, version, bucket, ordinal, \
             handler_id, kind, target_ref, command, label, timeout_ms, safe_kind, escalate_to) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                version,
                bucket,
                position,
                handler.id,
                columns.kind,
                columns.target,
                columns.command,
                columns.label,
                columns.timeout_ms,
                columns.safe_kind,
                columns.escalate_to
            ],
        )?;
        if let Some(exact) = &handler.target {
            cx.write(
                "INSERT INTO blueprint_handler_targets (blueprint_id, version, handler_id, \
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
    Ok(())
}

pub(super) fn insert_head(
    cx: &mut WriteCx<'_>,
    id: &str,
    version: u64,
) -> Result<(), StorageError> {
    cx.write(
        "INSERT INTO blueprint_heads (blueprint_id, version) VALUES (?1, ?2)",
        params![id, to_sql_u64("definition version", version)?],
    )?;
    Ok(())
}

pub(super) fn move_head(cx: &mut WriteCx<'_>, id: &str, version: u64) -> Result<(), StorageError> {
    cx.write(
        "UPDATE blueprint_heads SET version = ?2 WHERE blueprint_id = ?1",
        params![id, to_sql_u64("definition version", version)?],
    )?;
    Ok(())
}

pub(super) fn head_version(conn: &Connection, id: &str) -> Result<Option<u64>, StorageError> {
    let stored: Option<i64> = conn
        .prepare_cached("SELECT version FROM blueprint_heads WHERE blueprint_id = ?1")
        .and_then(|mut statement| statement.query_row([id], |row| row.get(0)).optional())
        .map_err(|e| sqlite_error("read sentinel head", e))?;
    stored
        .map(|version| from_sql_u64("definition version", version))
        .transpose()
}

pub(super) fn latest_version(conn: &Connection, id: &str) -> Result<u64, StorageError> {
    let latest: i64 = conn
        .prepare_cached(
            "SELECT COALESCE(MAX(version), 0) FROM blueprint_definitions WHERE blueprint_id = ?1",
        )
        .and_then(|mut statement| statement.query_row([id], |row| row.get(0)))
        .map_err(|e| sqlite_error("read sentinel versions", e))?;
    from_sql_u64("definition version", latest)
}

/// `(max_transitions, max_actions)` of the current head version.
pub(super) fn head_budgets(conn: &Connection, id: &str) -> Result<(u64, u64), StorageError> {
    conn.prepare_cached(
        "SELECT CASE WHEN d.session THEN ?2 ELSE d.max_transitions END, CASE WHEN d.session THEN ?2 ELSE d.max_actions END FROM blueprint_heads h \
         JOIN blueprint_definitions d ON d.blueprint_id = h.blueprint_id AND d.version = h.version \
         WHERE h.blueprint_id = ?1",
    )
    .and_then(|mut statement| {
        statement.query_row(params![id, codeterm_blueprint_types::blueprint_terminal::SESSION_SAFETY_LIMIT as i64], |row| {
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
            "SELECT hd.kind, hd.target_ref, hd.command, hd.label, hd.timeout_ms, hd.safe_kind, \
             hd.escalate_to FROM blueprint_heads h JOIN blueprint_handlers hd \
             ON hd.blueprint_id = h.blueprint_id AND hd.version = h.version \
             WHERE h.blueprint_id = ?1 AND hd.bucket = ?2 AND hd.handler_id = ?3",
        )
        .and_then(|mut statement| {
            statement
                .query_row(params![id, bucket, handler], |row| ActionRow::read(row, 0))
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel handler", e))?;
    row.map(ActionRow::action).transpose()
}

struct Header {
    model_classification: bool,
    terminal_spec: Option<String>,
    session: bool,
    provenance: String,
    name: Option<String>,
    source_kind: String,
    source_spec: Option<String>,
    pipeline_spec: Option<String>,
    probe: String,
    observe_kind: Option<String>,
    observe_ref: Option<String>,
    question: String,
    interval_ms: i64,
    until: Option<String>,
    escalation_timeout_ms: Option<i64>,
    max_transitions: i64,
    max_actions: i64,
    replaced_version: Option<i64>,
    author_tab_id: Option<String>,
    cause_transition_id: Option<String>,
    authored_at: Option<i64>,
    created_revision: i64,
}

pub(super) fn load_version(
    conn: &Connection,
    id: &str,
    version: u64,
) -> Result<StoredDefinition, StorageError> {
    let sql_version = to_sql_u64("definition version", version)?;
    let header = conn
        .prepare_cached(
            "SELECT provenance, name, source_kind, source_spec, pipeline_spec, probe, observe_kind, \
             observe_ref, question, interval_ms, until_bucket, escalation_timeout_ms, \
             max_transitions, max_actions, replaced_version, author_tab_id, \
             cause_transition_id, authored_at, created_revision, terminal_spec, session, model_classification \
             FROM blueprint_definitions WHERE blueprint_id = ?1 AND version = ?2",
        )
        .and_then(|mut statement| {
            statement
                .query_row(params![id, sql_version], |row| {
                    Ok(Header {
                        provenance: row.get(0)?,
                        name: row.get(1)?,
                        source_kind: row.get(2)?,
                        source_spec: row.get(3)?,
                        pipeline_spec: row.get(4)?,
                        probe: row.get(5)?,
                        observe_kind: row.get(6)?,
                        observe_ref: row.get(7)?,
                        question: row.get(8)?,
                        interval_ms: row.get(9)?,
                        until: row.get(10)?,
                        escalation_timeout_ms: row.get(11)?,
                        max_transitions: row.get(12)?,
                        max_actions: row.get(13)?,
                        replaced_version: row.get(14)?,
                        author_tab_id: row.get(15)?,
                        cause_transition_id: row.get(16)?,
                        authored_at: row.get(17)?,
                        created_revision: row.get(18)?,
                        terminal_spec: row.get(19)?,
                        session: row.get(20)?,
                        model_classification: row.get(21)?,
                    })
                })
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel definition", e))?
        .ok_or_else(|| corrupt("head names a missing version"))?;
    let budget = |field: &'static str, value: i64| u16::try_from(value).map_err(|_| corrupt(field));
    let (buckets, on_other) = load_buckets(conn, id, sql_version)?;
    let observe = observe_from(header.observe_kind, header.observe_ref)?;
    let pipeline = header
        .pipeline_spec
        .map(|spec| {
            serde_json::from_str(&spec).map_err(|_| corrupt("pipeline spec does not decode"))
        })
        .transpose()?;
    let terminal = header
        .terminal_spec
        .map(|spec| {
            serde_json::from_str(&spec).map_err(|_| corrupt("terminal spec does not decode"))
        })
        .transpose()?;
    Ok(StoredDefinition {
        version,
        provenance: provenance_from(&header.provenance)?,
        created_revision: Revision(from_sql_u64("revision", header.created_revision)?),
        amendment: amendment_from(
            version,
            header.replaced_version,
            header.author_tab_id,
            header.cause_transition_id,
            header.authored_at,
        )?,
        definition: StageDefinition {
            model_classification: header.model_classification,
            session: header.session,
            name: header.name,
            source: source_from(
                &header.source_kind,
                header.source_spec,
                header.probe,
                observe,
                terminal,
                pipeline,
            )?,
            question: header.question,
            interval_ms: from_sql_u64("sentinel interval", header.interval_ms)?,
            until: header.until,
            escalation_timeout_ms: header
                .escalation_timeout_ms
                .map(|timeout| from_sql_u64("escalation timeout", timeout))
                .transpose()?,
            max_transitions: budget("max transitions", header.max_transitions)?,
            max_actions: budget("max actions", header.max_actions)?,
            buckets,
            on_other,
        },
    })
}

/// Declared buckets in order, plus the handlers of the reserved `other` bucket.
fn load_buckets(
    conn: &Connection,
    id: &str,
    version: i64,
) -> Result<(Vec<BucketDefinition>, Vec<HandlerDefinition>), StorageError> {
    let mut buckets: Vec<BucketDefinition> = {
        let mut statement = conn
            .prepare_cached(
                "SELECT bucket, match_pattern, rearm_on_change FROM blueprint_buckets \
                 WHERE blueprint_id = ?1 AND version = ?2 ORDER BY ordinal",
            )
            .map_err(|e| sqlite_error("read sentinel buckets", e))?;
        let rows = statement
            .query_map(params![id, version], |row| {
                Ok(BucketDefinition {
                    name: row.get(0)?,
                    match_pattern: row.get(1)?,
                    rearm_on_change: row.get(2)?,
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
            "SELECT bucket, handler_id, kind, target_ref, command, label, timeout_ms, \
             safe_kind, escalate_to FROM blueprint_handlers \
             WHERE blueprint_id = ?1 AND version = ?2 ORDER BY bucket, ordinal",
        )
        .map_err(|e| sqlite_error("read sentinel handlers", e))?;
    let rows = statement
        .query_map(params![id, version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                ActionRow::read(row, 2)?,
            ))
        })
        .map_err(|e| sqlite_error("read sentinel handlers", e))?;
    for row in rows {
        let (bucket, handler, action) =
            row.map_err(|e| sqlite_error("read sentinel handlers", e))?;
        let position = *index
            .get(&bucket)
            .ok_or_else(|| corrupt("handler names an undeclared bucket"))?;
        buckets[position].handlers.push(HandlerDefinition {
            target: targets.remove(&handler),
            action: action.action()?,
            id: handler,
        });
    }
    if !targets.is_empty() {
        return Err(corrupt("target names a missing handler"));
    }
    let on_other = match buckets
        .iter()
        .position(|bucket| bucket.name == OTHER_BUCKET)
    {
        Some(position) => {
            let other = buckets.remove(position);
            if other.handlers.is_empty() || other.match_pattern.is_some() {
                return Err(corrupt("reserved other bucket is not a handler list"));
            }
            other.handlers
        }
        None => Vec::new(),
    };
    Ok((buckets, on_other))
}

fn load_targets(
    conn: &Connection,
    id: &str,
    version: i64,
) -> Result<HashMap<String, TargetIdentity>, StorageError> {
    let mut statement = conn
        .prepare_cached(
            "SELECT handler_id, kind, target_id, workspace_id, team_id, owner_tab_id \
             FROM blueprint_handler_targets WHERE blueprint_id = ?1 AND version = ?2",
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

/// The newest amended versions up to `head`, oldest first and bounded by the lineage window.
pub(super) fn load_lineage(
    conn: &Connection,
    id: &str,
    head: u64,
) -> Result<Vec<AmendmentRecord>, StorageError> {
    let limit = i64::try_from(MAX_SENTINEL_LINEAGE).map_err(|_| corrupt("lineage window"))?;
    let mut statement = conn
        .prepare_cached(
            "SELECT version, replaced_version, author_tab_id, cause_transition_id, authored_at \
             FROM blueprint_definitions WHERE blueprint_id = ?1 AND version <= ?2 \
             AND replaced_version IS NOT NULL ORDER BY version DESC LIMIT ?3",
        )
        .map_err(|e| sqlite_error("read sentinel lineage", e))?;
    let rows = statement
        .query_map(
            params![id, to_sql_u64("definition version", head)?, limit],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .map_err(|e| sqlite_error("read sentinel lineage", e))?;
    let mut lineage = Vec::new();
    for row in rows {
        let (version, replaced, author, cause, authored_at) =
            row.map_err(|e| sqlite_error("read sentinel lineage", e))?;
        let version = from_sql_u64("definition version", version)?;
        lineage.push(
            amendment_from(version, replaced, author, cause, authored_at)?
                .ok_or_else(|| corrupt("amended version lacks its lineage"))?,
        );
    }
    lineage.reverse();
    Ok(lineage)
}

pub(super) fn load_history(
    conn: &Connection,
    id: &str,
) -> Result<Vec<StoredDefinition>, StorageError> {
    let versions: Vec<i64> = {
        let mut statement = conn
            .prepare_cached(
                "SELECT version FROM blueprint_definitions WHERE blueprint_id = ?1 ORDER BY version",
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
