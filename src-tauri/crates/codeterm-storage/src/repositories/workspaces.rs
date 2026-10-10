use crate::assets::{self, AssetId, AssetRef};
use crate::error::{sqlite_error, StorageError};
use crate::owner::{Storage, WriteCx};
use crate::repositories::{
    expect_matches, from_sql_u64, to_sql_u64, Committed, Completion, Expect, Read, Revision,
};
use crate::schema::{self, WORKSPACE_ORDER_REVISION};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};

pub const MAX_WORKSPACES: usize = 4096;
pub const MAX_UPSERT_BATCH: usize = 256;
const MAX_ID_BYTES: usize = 256;
const MAX_NAME_BYTES: usize = 1024;
const MAX_PATH_BYTES: usize = 32 * 1024;
const MAX_PREFIX_BYTES: usize = 64;
const MAX_ROLE_BYTES: usize = 64;
const RECORD_OVERHEAD: usize = 64;

const SELECT_WORKSPACES: &str = "SELECT id, name, cwd, provisioned_cwd, protected, \
     task_key_prefix, task_key_next, revision FROM workspaces";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    pub fn new(id: impl Into<String>) -> Result<Self, StorageError> {
        let id = id.into();
        if id.is_empty() || id.len() > MAX_ID_BYTES || id.chars().any(char::is_control) {
            return Err(StorageError::invalid(
                "workspace id",
                "must be 1..=256 bytes without control characters",
            ));
        }
        Ok(WorkspaceId(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Name of one asset slot on a workspace: `[a-z0-9][a-z0-9._-]{0,63}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetRole(String);

impl AssetRole {
    pub fn new(role: impl Into<String>) -> Result<Self, StorageError> {
        let role = role.into();
        let allowed =
            |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-');
        let valid = role.len() <= MAX_ROLE_BYTES
            && role
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && role.bytes().all(allowed);
        if !valid {
            return Err(StorageError::invalid(
                "asset role",
                "must match [a-z0-9][a-z0-9._-]{0,63}",
            ));
        }
        Ok(AssetRole(role))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Durable scalar fields of one workspace; tabs, Teams and layout own separate records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRecord {
    pub id: WorkspaceId,
    pub name: String,
    pub cwd: Option<String>,
    pub provisioned_cwd: Option<String>,
    pub protected: bool,
    pub task_key_prefix: Option<String>,
    pub task_key_next: u64,
}

impl WorkspaceRecord {
    pub fn new(id: WorkspaceId, name: impl Into<String>) -> Self {
        WorkspaceRecord {
            id,
            name: name.into(),
            cwd: None,
            provisioned_cwd: None,
            protected: false,
            task_key_prefix: None,
            task_key_next: 0,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        bounded("workspace name", &self.name, MAX_NAME_BYTES)?;
        for (field, value) in [
            ("workspace cwd", &self.cwd),
            ("provisioned cwd", &self.provisioned_cwd),
        ] {
            if let Some(value) = value {
                bounded(field, value, MAX_PATH_BYTES)?;
            }
        }
        if let Some(prefix) = &self.task_key_prefix {
            bounded("task key prefix", prefix, MAX_PREFIX_BYTES)?;
        }
        to_sql_u64("task key counter", self.task_key_next).map(|_| ())
    }

    pub(crate) fn cost(&self) -> usize {
        let optional = |value: &Option<String>| value.as_ref().map_or(0, String::len);
        RECORD_OVERHEAD
            + self.id.0.len()
            + self.name.len()
            + optional(&self.cwd)
            + optional(&self.provisioned_cwd)
            + optional(&self.task_key_prefix)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceUpsert {
    pub record: WorkspaceRecord,
    pub expect: Expect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAsset {
    pub role: AssetRole,
    pub asset: AssetRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredWorkspace {
    pub record: WorkspaceRecord,
    pub revision: Revision,
    pub assets: Vec<WorkspaceAsset>,
}

/// Every workspace in durable order, with the order's own revision for reorders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInventory {
    pub workspaces: Vec<StoredWorkspace>,
    pub order_revision: Revision,
}

/// Typed incremental workspace rows. Callers submit already-authorized domain operations.
#[derive(Clone)]
pub struct WorkspaceRepository {
    storage: Storage,
}

impl WorkspaceRepository {
    pub(crate) fn new(storage: Storage) -> Self {
        WorkspaceRepository { storage }
    }

    pub fn list(&self) -> Completion<Read<WorkspaceInventory>> {
        self.storage.submit_read(|cx| load_inventory(cx.conn))
    }

    pub fn get(&self, id: WorkspaceId) -> Completion<Read<Option<StoredWorkspace>>> {
        self.storage.submit_read(move |cx| load_one(cx.conn, &id))
    }

    /// Inserts (`Expect::Absent`, appended in batch order) or updates only the supplied
    /// records; any stale expectation rejects the whole batch.
    pub fn upsert(&self, batch: Vec<WorkspaceUpsert>) -> Completion<Committed<()>> {
        if let Err(error) = validate_batch(&batch) {
            return Completion::settled(Err(error));
        }
        let cost = batch.iter().map(|item| item.record.cost()).sum();
        self.storage
            .submit_mutation(cost, move |cx| apply_upsert(cx, batch))
    }

    pub fn rename(
        &self,
        id: WorkspaceId,
        expect: Revision,
        name: String,
    ) -> Completion<Committed<()>> {
        if let Err(error) = bounded("workspace name", &name, MAX_NAME_BYTES) {
            return Completion::settled(Err(error));
        }
        let cost = RECORD_OVERHEAD + id.0.len() + name.len();
        self.storage.submit_mutation(cost, move |cx| {
            expect_matches(
                &entity(&id),
                Expect::At(expect),
                row_revision(cx.conn, &id)?,
            )?;
            let revision = sql_revision(cx)?;
            cx.write(
                "UPDATE workspaces SET name = ?2, revision = ?3 WHERE id = ?1",
                params![id.as_str(), name, revision],
            )?;
            Ok(())
        })
    }

    /// Applies an exact permutation of the current workspaces against the order revision.
    pub fn reorder(
        &self,
        expect_order: Revision,
        order: Vec<WorkspaceId>,
    ) -> Completion<Committed<()>> {
        if order.len() > MAX_WORKSPACES {
            return Completion::settled(Err(StorageError::invalid(
                "workspace order",
                "exceeds the inventory bound",
            )));
        }
        let cost = RECORD_OVERHEAD + order.iter().map(|id| id.0.len()).sum::<usize>();
        self.storage
            .submit_mutation(cost, move |cx| apply_reorder(cx, expect_order, &order))
    }

    /// Refused by the foreign key while assets are attached; nothing is detached implicitly.
    pub fn remove(&self, id: WorkspaceId, expect: Revision) -> Completion<Committed<()>> {
        self.storage
            .submit_mutation(RECORD_OVERHEAD + id.0.len(), move |cx| {
                expect_matches(
                    &entity(&id),
                    Expect::At(expect),
                    row_revision(cx.conn, &id)?,
                )?;
                cx.write("DELETE FROM workspaces WHERE id = ?1", [id.as_str()])?;
                bump_order(cx)
            })
    }

    /// Publishes the bytes as an immutable asset, then commits the reference; a replaced
    /// asset is released once nothing references it.
    pub fn attach_asset(
        &self,
        id: WorkspaceId,
        expect: Revision,
        role: AssetRole,
        bytes: Vec<u8>,
    ) -> Completion<Committed<AssetRef>> {
        let cost = RECORD_OVERHEAD + id.0.len() + role.0.len() + bytes.len();
        self.storage.submit_mutation(cost, move |cx| {
            expect_matches(
                &entity(&id),
                Expect::At(expect),
                row_revision(cx.conn, &id)?,
            )?;
            let previous = attached(cx.conn, &id, &role)?;
            let asset = assets::publish(cx, &bytes)?;
            let sql = if previous.is_some() {
                "UPDATE workspace_assets SET asset_id = ?3 WHERE workspace_id = ?1 AND role = ?2"
            } else {
                "INSERT INTO workspace_assets (workspace_id, role, asset_id) VALUES (?1, ?2, ?3)"
            };
            cx.write(sql, params![id.as_str(), role.as_str(), asset.id.as_str()])?;
            if let Some(previous) = previous {
                assets::release_if_unreferenced(cx, &previous)?;
            }
            touch(cx, &id)?;
            Ok(asset)
        })
    }

    /// Commits the reference removal first; the file is removed after COMMIT.
    pub fn detach_asset(
        &self,
        id: WorkspaceId,
        expect: Revision,
        role: AssetRole,
    ) -> Completion<Committed<AssetId>> {
        let cost = RECORD_OVERHEAD + id.0.len() + role.0.len();
        self.storage.submit_mutation(cost, move |cx| {
            expect_matches(
                &entity(&id),
                Expect::At(expect),
                row_revision(cx.conn, &id)?,
            )?;
            let asset = attached(cx.conn, &id, &role)?.ok_or_else(|| StorageError::NotFound {
                entity: format!("{} asset {}", entity(&id), role.as_str()),
            })?;
            cx.write(
                "DELETE FROM workspace_assets WHERE workspace_id = ?1 AND role = ?2",
                params![id.as_str(), role.as_str()],
            )?;
            assets::release_if_unreferenced(cx, &asset)?;
            touch(cx, &id)?;
            Ok(asset)
        })
    }

    pub fn asset(&self, id: WorkspaceId, role: AssetRole) -> Completion<Read<Option<Vec<u8>>>> {
        self.storage.submit_read(move |cx| {
            let found: Option<(String, i64)> = cx
                .conn
                .query_row(
                    "SELECT a.id, a.byte_len FROM workspace_assets wa \
                     JOIN assets a ON a.id = wa.asset_id \
                     WHERE wa.workspace_id = ?1 AND wa.role = ?2",
                    params![id.as_str(), role.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| sqlite_error("read workspace asset", e))?;
            let Some((asset, byte_len)) = found else {
                return Ok(None);
            };
            let asset = parse_asset(&asset)?;
            cx.before_asset_file();
            assets::read_blob(cx.layout, &asset, from_sql_u64("asset size", byte_len)?).map(Some)
        })
    }
}

fn bounded(field: &'static str, value: &str, limit: usize) -> Result<(), StorageError> {
    if value.len() > limit {
        return Err(StorageError::invalid(
            field,
            format!("exceeds {limit} bytes"),
        ));
    }
    Ok(())
}

fn validate_batch(batch: &[WorkspaceUpsert]) -> Result<(), StorageError> {
    if batch.is_empty() || batch.len() > MAX_UPSERT_BATCH {
        return Err(StorageError::invalid(
            "workspace batch",
            format!("must hold 1..={MAX_UPSERT_BATCH} records"),
        ));
    }
    let mut seen = HashSet::new();
    for item in batch {
        item.record.validate()?;
        if !seen.insert(item.record.id.as_str()) {
            return Err(StorageError::invalid(
                "workspace batch",
                "repeats a workspace id",
            ));
        }
    }
    Ok(())
}

fn entity(id: &WorkspaceId) -> String {
    format!("workspace {}", id.0)
}

fn sql_revision(cx: &WriteCx<'_>) -> Result<i64, StorageError> {
    to_sql_u64("revision", cx.revision.0)
}

fn row_revision(conn: &Connection, id: &WorkspaceId) -> Result<Option<Revision>, StorageError> {
    let stored: Option<i64> = conn
        .prepare_cached("SELECT revision FROM workspaces WHERE id = ?1")
        .and_then(|mut statement| {
            statement
                .query_row([id.as_str()], |row| row.get(0))
                .optional()
        })
        .map_err(|e| sqlite_error("read workspace revision", e))?;
    stored
        .map(|value| from_sql_u64("revision", value).map(Revision))
        .transpose()
}

fn touch(cx: &mut WriteCx<'_>, id: &WorkspaceId) -> Result<(), StorageError> {
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE workspaces SET revision = ?2 WHERE id = ?1",
        params![id.as_str(), revision],
    )?;
    Ok(())
}

fn bump_order(cx: &WriteCx<'_>) -> Result<(), StorageError> {
    schema::write_meta(cx.conn, WORKSPACE_ORDER_REVISION, cx.revision.0)
}

fn attached(
    conn: &Connection,
    id: &WorkspaceId,
    role: &AssetRole,
) -> Result<Option<AssetId>, StorageError> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT asset_id FROM workspace_assets WHERE workspace_id = ?1 AND role = ?2",
            params![id.as_str(), role.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| sqlite_error("read workspace asset", e))?;
    stored.map(|asset| parse_asset(&asset)).transpose()
}

fn parse_asset(text: &str) -> Result<AssetId, StorageError> {
    AssetId::parse(text).map_err(|_| StorageError::Corrupt {
        detail: "malformed asset id".to_string(),
    })
}

fn apply_upsert(cx: &mut WriteCx<'_>, batch: Vec<WorkspaceUpsert>) -> Result<(), StorageError> {
    let revision = sql_revision(cx)?;
    let mut appended = false;
    for WorkspaceUpsert { record, expect } in batch {
        let current = row_revision(cx.conn, &record.id)?;
        expect_matches(&entity(&record.id), expect, current)?;
        let counter = to_sql_u64("task key counter", record.task_key_next)?;
        let id = record.id.as_str();
        let values = params![
            id,
            record.name,
            record.cwd,
            record.provisioned_cwd,
            record.protected,
            record.task_key_prefix,
            counter,
            revision
        ];
        if current.is_some() {
            cx.write(
                "UPDATE workspaces SET name = ?2, cwd = ?3, provisioned_cwd = ?4, protected = ?5, \
                 task_key_prefix = ?6, task_key_next = ?7, revision = ?8 WHERE id = ?1",
                values,
            )?;
            continue;
        }
        let count: i64 = cx
            .conn
            .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
            .map_err(|e| sqlite_error("count workspaces", e))?;
        if count >= MAX_WORKSPACES as i64 {
            return Err(StorageError::invalid(
                "workspaces",
                "inventory is at its bound",
            ));
        }
        cx.write(
            "INSERT INTO workspaces (id, ordinal, name, cwd, provisioned_cwd, protected, \
             task_key_prefix, task_key_next, revision) \
             VALUES (?1, (SELECT COALESCE(MAX(ordinal), -1) + 1 FROM workspaces), \
             ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            values,
        )?;
        appended = true;
    }
    if appended {
        bump_order(cx)?;
    }
    Ok(())
}

fn apply_reorder(
    cx: &mut WriteCx<'_>,
    expect_order: Revision,
    order: &[WorkspaceId],
) -> Result<(), StorageError> {
    let current_order = Revision(schema::read_meta(cx.conn, WORKSPACE_ORDER_REVISION)?);
    expect_matches(
        "workspace order",
        Expect::At(expect_order),
        Some(current_order),
    )?;
    let current = ordinals(cx.conn)?;
    let known: HashMap<&str, i64> = current
        .iter()
        .map(|(id, ordinal)| (id.as_str(), *ordinal))
        .collect();
    let mut seen = HashSet::new();
    let exact = order.len() == current.len()
        && order
            .iter()
            .all(|id| known.contains_key(id.as_str()) && seen.insert(id.as_str()));
    if !exact {
        return Err(StorageError::invalid(
            "workspace order",
            "must be an exact permutation of the current workspaces",
        ));
    }
    let moved: Vec<(&str, i64)> = order
        .iter()
        .zip(0_i64..)
        .filter(|(id, target)| known.get(id.as_str()) != Some(target))
        .map(|(id, target)| (id.as_str(), target))
        .collect();
    if moved.is_empty() {
        return Ok(());
    }
    // Two phases keep UNIQUE(ordinal) satisfied: park moved rows on negative ordinals first.
    for (id, _) in &moved {
        cx.conn
            .execute(
                "UPDATE workspaces SET ordinal = -1 - ordinal WHERE id = ?1",
                [id],
            )
            .map_err(|e| sqlite_error("park workspace ordinal", e))?;
    }
    for (id, target) in &moved {
        cx.write(
            "UPDATE workspaces SET ordinal = ?2 WHERE id = ?1",
            params![id, target],
        )?;
    }
    bump_order(cx)
}

fn ordinals(conn: &Connection) -> Result<Vec<(String, i64)>, StorageError> {
    let mut statement = conn
        .prepare_cached("SELECT id, ordinal FROM workspaces ORDER BY ordinal")
        .map_err(|e| sqlite_error("read workspace order", e))?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| sqlite_error("read workspace order", e))?;
    rows.map(|row| row.map_err(|e| sqlite_error("read workspace order", e)))
        .collect()
}

type RawWorkspace = (
    String,
    String,
    Option<String>,
    Option<String>,
    bool,
    Option<String>,
    i64,
    i64,
);

fn raw_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawWorkspace> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn decode(raw: RawWorkspace, assets: Vec<WorkspaceAsset>) -> Result<StoredWorkspace, StorageError> {
    let (id, name, cwd, provisioned_cwd, protected, task_key_prefix, next, revision) = raw;
    let id = WorkspaceId::new(id).map_err(|_| StorageError::Corrupt {
        detail: "malformed workspace id".to_string(),
    })?;
    Ok(StoredWorkspace {
        record: WorkspaceRecord {
            id,
            name,
            cwd,
            provisioned_cwd,
            protected,
            task_key_prefix,
            task_key_next: from_sql_u64("task key counter", next)?,
        },
        revision: Revision(from_sql_u64("revision", revision)?),
        assets,
    })
}

fn load_assets(
    conn: &Connection,
    only: Option<&WorkspaceId>,
) -> Result<HashMap<String, Vec<WorkspaceAsset>>, StorageError> {
    let mut statement = conn
        .prepare_cached(
            "SELECT wa.workspace_id, wa.role, a.id, a.byte_len FROM workspace_assets wa \
             JOIN assets a ON a.id = wa.asset_id WHERE ?1 IS NULL OR wa.workspace_id = ?1 \
             ORDER BY wa.workspace_id, wa.role",
        )
        .map_err(|e| sqlite_error("read workspace assets", e))?;
    let rows = statement
        .query_map([only.map(WorkspaceId::as_str)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| sqlite_error("read workspace assets", e))?;
    let mut grouped: HashMap<String, Vec<WorkspaceAsset>> = HashMap::new();
    for row in rows {
        let (workspace, role, asset, byte_len) =
            row.map_err(|e| sqlite_error("read workspace assets", e))?;
        let role = AssetRole::new(role).map_err(|_| StorageError::Corrupt {
            detail: "malformed asset role".to_string(),
        })?;
        let asset = AssetRef {
            id: parse_asset(&asset)?,
            byte_len: from_sql_u64("asset size", byte_len)?,
        };
        grouped
            .entry(workspace)
            .or_default()
            .push(WorkspaceAsset { role, asset });
    }
    Ok(grouped)
}

fn load_inventory(conn: &Connection) -> Result<WorkspaceInventory, StorageError> {
    let mut attachments = load_assets(conn, None)?;
    let mut statement = conn
        .prepare_cached(&format!("{SELECT_WORKSPACES} ORDER BY ordinal"))
        .map_err(|e| sqlite_error("read workspaces", e))?;
    let rows = statement
        .query_map([], raw_row)
        .map_err(|e| sqlite_error("read workspaces", e))?;
    let mut workspaces = Vec::new();
    for row in rows {
        let raw = row.map_err(|e| sqlite_error("read workspaces", e))?;
        let assets = attachments.remove(&raw.0).unwrap_or_default();
        workspaces.push(decode(raw, assets)?);
    }
    let order_revision = Revision(schema::read_meta(conn, WORKSPACE_ORDER_REVISION)?);
    Ok(WorkspaceInventory {
        workspaces,
        order_revision,
    })
}

fn load_one(conn: &Connection, id: &WorkspaceId) -> Result<Option<StoredWorkspace>, StorageError> {
    let raw = conn
        .prepare_cached(&format!("{SELECT_WORKSPACES} WHERE id = ?1"))
        .and_then(|mut statement| statement.query_row([id.as_str()], raw_row).optional())
        .map_err(|e| sqlite_error("read workspace", e))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let assets = load_assets(conn, Some(id))?
        .remove(id.as_str())
        .unwrap_or_default();
    decode(raw, assets).map(Some)
}
