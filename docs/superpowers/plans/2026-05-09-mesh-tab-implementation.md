# Mesh Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the unified Mesh tab — a single panel rendering crons, daemons, and mesh-wide service visibility as terminals-as-terminals canvas + flat services table, with per-service Bearer-token push API and absorbed peer-management settings.

**Architecture:** Backend `services::store` (rusqlite, mirrors `mem::store` modular split) layers on top of unchanged `cron::store` via virtual-Service projection. New `MeshPanel` frontend (mirrors `MemoryPanel` aesthetic exactly) consumes services via Tauri invoke + REST. Pull-on-tab-open mesh sync; per-service Bearer tokens for `POST /api/panes/<id>/send` extension; bounded SQLite activity log. Settings reorganization moves Memory and Mesh sections into their respective tabs.

**Tech Stack:** Rust + Tauri v2 backend (rusqlite bundled, parking_lot, axum REST, tokio for background probe scheduler), React + TypeScript frontend (zustand-style hooks, Sigma + physics worker reused from MemoryPanel), `clap` CLI extension.

**Spec reference:** `docs/superpowers/specs/2026-05-09-mesh-tab-design.md` (commit 11a5dcb on branch `feature/v1.1.25-mesh-tab-design`)

---

## How to use this plan

The plan is organized into 16 phases mirroring spec §8.4 migration ordering. Each phase contains 1-5 tasks. Phases 1-3 are pure backend additions (no UI changes). Phases 4-9 add the new frontend tab incrementally. Phase 10 is the breaking Settings change. Phases 11-15 are polish + final cleanup. Phase 16 is documentation.

**Each step is a single 2-5 minute action.** Run tests after every implementation step. Commit after each task. If a step fails, do not proceed — diagnose and fix.

**Branch:** All work on `feature/v1.1.25-mesh-tab-design` (already checked out). Do NOT touch the v1.1.24 train branch (`fix/v1.1.24-alias-prefix-whitelist`).

**File-size discipline:** Hard limit 800 lines per source file (`scripts/check-file-sizes.sh` enforced). Split when approaching.

**Lock ordering (deadlock prevention):** mux → output_buf → screen_state → agent_tracker → pty. Never acquire in reverse. parking_lot is non-reentrant.

---

## Phase 1: Backend services-store

### Task 1.1: Create services::store module skeleton + schema

**Files:**
- Create: `src-tauri/src/services/mod.rs`
- Create: `src-tauri/src/services/store/mod.rs`
- Create: `src-tauri/src/services/store/schema.rs`
- Create: `src-tauri/src/services/store/types.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod services;`)
- Test: `src-tauri/src/services/store/tests.rs`

- [ ] **Step 1: Create `services/mod.rs` facade**

```rust
//! Services subsystem — registry of cron + daemon services with health probing,
//! activity log, and per-service push API token middleware.
//!
//! Layout:
//!   - [`store`]    — Service entity persistence (rusqlite)
//!   - [`health`]   — health probe (4 modes) + cache
//!   - [`push`]     — Bearer-token middleware for /api/panes/:id/send
//!   - [`activity`] — bounded activity log
//!   - [`tokens`]   — file-based per-service token store (~/.codeterm/services/<id>/token)
//!   - [`smart_add`]— AI-agent-assisted field mapping wizard backend

pub mod store;
```

- [ ] **Step 2: Create `services/store/types.rs` with all structs**

```rust
//! Service entity types — single source of truth for backend + REST + frontend.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceKind {
    Cron,
    Daemon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthMode {
    Http,
    Tcp,
    Process,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Down,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HealthSnapshot {
    pub status: HealthStatus,
    pub probed_at: Option<i64>,
    pub latency_ms: Option<u64>,
    pub version: Option<String>,
    pub uptime_secs: Option<u64>,
    pub extra: serde_json::Value,
}

impl Default for HealthStatus {
    fn default() -> Self { HealthStatus::Unknown }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldMapping {
    pub mappings: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub kind: ServiceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_path: Option<String>,
    pub health_mode: HealthMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_path: Option<String>,
    pub health_interval: u32,
    pub health_ttl: u32,
    pub health: HealthSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_mapping: Option<FieldMapping>,
    pub push_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_status: Option<String>,
    pub registered_at: i64,
    pub updated_at: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceCreate {
    pub name: String,
    pub kind: ServiceKind,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub process_label: Option<String>,
    #[serde(default)]
    pub script_path: Option<String>,
    pub health_mode: HealthMode,
    #[serde(default)]
    pub health_path: Option<String>,
    #[serde(default)]
    pub health_interval: Option<u32>,
    #[serde(default)]
    pub field_mapping: Option<FieldMapping>,
    #[serde(default)]
    pub push_enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceUpdate {
    #[serde(default)] pub name: Option<String>,
    #[serde(default)] pub description: Option<String>,
    #[serde(default)] pub url: Option<String>,
    #[serde(default)] pub host: Option<String>,
    #[serde(default)] pub port: Option<u16>,
    #[serde(default)] pub process_label: Option<String>,
    #[serde(default)] pub script_path: Option<String>,
    #[serde(default)] pub health_mode: Option<HealthMode>,
    #[serde(default)] pub health_path: Option<String>,
    #[serde(default)] pub health_interval: Option<u32>,
    #[serde(default)] pub field_mapping: Option<FieldMapping>,
    #[serde(default)] pub push_enabled: Option<bool>,
    #[serde(default)] pub enabled: Option<bool>,
}
```

- [ ] **Step 3: Create `services/store/schema.rs` with SQL DDL**

```rust
//! services.db schema initialization.

use rusqlite::Connection;

pub const SCHEMA_VERSION: u32 = 1;

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS services (
            id              TEXT PRIMARY KEY,
            name            TEXT NOT NULL,
            kind            TEXT NOT NULL,
            description     TEXT,
            url             TEXT,
            host            TEXT,
            port            INTEGER,
            process_label   TEXT,
            script_path     TEXT,
            health_mode     TEXT NOT NULL,
            health_path     TEXT,
            health_interval INTEGER NOT NULL DEFAULT 30,
            health_ttl      INTEGER NOT NULL DEFAULT 60,
            field_mapping   TEXT,
            push_enabled    BOOLEAN NOT NULL DEFAULT 0,
            registered_at   INTEGER NOT NULL,
            updated_at      INTEGER NOT NULL,
            enabled         BOOLEAN NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_services_kind ON services(kind);
        CREATE INDEX IF NOT EXISTS idx_services_enabled ON services(enabled);

        CREATE TABLE IF NOT EXISTS service_health (
            service_id      TEXT NOT NULL,
            probed_at       INTEGER NOT NULL,
            status          TEXT NOT NULL,
            response        TEXT,
            latency_ms      INTEGER,
            PRIMARY KEY (service_id, probed_at),
            FOREIGN KEY (service_id) REFERENCES services(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_health_service_time
            ON service_health(service_id, probed_at DESC);

        CREATE TABLE IF NOT EXISTS activity (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            occurred_at     INTEGER NOT NULL,
            source          TEXT NOT NULL,
            source_id       TEXT,
            target          TEXT,
            level           TEXT NOT NULL,
            message         TEXT NOT NULL,
            meta            TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_activity_time ON activity(occurred_at DESC);
        CREATE INDEX IF NOT EXISTS idx_activity_source ON activity(source, occurred_at DESC);

        CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
    "#)?;

    let mut stmt = conn.prepare("SELECT version FROM schema_version LIMIT 1")?;
    let exists: bool = stmt.exists([])?;
    if !exists {
        conn.execute("INSERT INTO schema_version (version) VALUES (?1)", [SCHEMA_VERSION])?;
    }
    Ok(())
}
```

- [ ] **Step 4: Create `services/store/mod.rs` with `ServicesStore` struct**

```rust
//! Service registry persistence — types, schema, and CRUD modules.

mod schema;
mod types;
mod crud;
#[cfg(test)]
mod tests;

pub use types::*;
pub use schema::{init_schema, SCHEMA_VERSION};

use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;

pub struct ServicesStore {
    conn: Arc<Mutex<Connection>>,
}

impl ServicesStore {
    pub fn open(path: &PathBuf) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        init_schema(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        init_schema(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub(crate) fn conn(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }
}
```

- [ ] **Step 5: Add `pub mod services;` to `src-tauri/src/lib.rs`**

Find the existing module declaration block (alongside `pub mod cron;`, `pub mod mesh;`, etc) and add:
```rust
pub mod services;
```

- [ ] **Step 6: Run `cargo check` to verify schema compiles**

```bash
cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tail -10
```
Expected: 0 errors. Warnings about unused store struct are acceptable at this stage.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/services/ src-tauri/src/lib.rs
git commit -m "feat(services): module skeleton + sqlite schema (services, service_health, activity)"
```

### Task 1.2: Implement CRUD operations

**Files:**
- Create: `src-tauri/src/services/store/crud.rs`
- Modify: `src-tauri/src/services/store/tests.rs` (add tests)

- [ ] **Step 1: Write failing test for create + get**

```rust
// src-tauri/src/services/store/tests.rs
use super::*;

#[test]
fn create_and_get_daemon_service() {
    let store = ServicesStore::open_in_memory().unwrap();
    let id = store.create(ServiceCreate {
        name: "voice-transcribe".into(),
        kind: ServiceKind::Daemon,
        description: Some("local whisper".into()),
        host: None,
        url: Some("http://127.0.0.1:7891".into()),
        port: None,
        process_label: None,
        script_path: None,
        health_mode: HealthMode::Http,
        health_path: Some("/healthz".into()),
        health_interval: Some(30),
        field_mapping: None,
        push_enabled: Some(true),
    }).unwrap();

    let svc = store.get(&id).unwrap().expect("service exists");
    assert_eq!(svc.name, "voice-transcribe");
    assert_eq!(svc.kind, ServiceKind::Daemon);
    assert_eq!(svc.url.as_deref(), Some("http://127.0.0.1:7891"));
    assert_eq!(svc.health_mode, HealthMode::Http);
    assert!(svc.push_enabled);
    assert!(svc.enabled);
}
```

- [ ] **Step 2: Run test, verify FAIL**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib services::store::tests::create_and_get_daemon_service 2>&1 | tail -10
```
Expected: FAIL — `no method named create on ServicesStore`.

- [ ] **Step 3: Implement `crud.rs` with `create` and `get`**

```rust
//! Service CRUD operations.

use rusqlite::{params, OptionalExtension};
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::*;
use super::ServicesStore;

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

impl ServicesStore {
    pub fn create(&self, input: ServiceCreate) -> rusqlite::Result<String> {
        let id = format!("svc_{}_{}", slug(&input.name), &uuid::Uuid::new_v4().to_string()[..8]);
        let now = now_ms();
        let conn = self.conn();
        let conn = conn.lock();
        conn.execute(
            "INSERT INTO services (
                id, name, kind, description, url, host, port,
                process_label, script_path, health_mode, health_path,
                health_interval, health_ttl, field_mapping, push_enabled,
                registered_at, updated_at, enabled
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
             )",
            params![
                id,
                input.name,
                serde_json::to_string(&input.kind).unwrap().trim_matches('"').to_string(),
                input.description,
                input.url,
                input.host,
                input.port,
                input.process_label,
                input.script_path,
                serde_json::to_string(&input.health_mode).unwrap().trim_matches('"').to_string(),
                input.health_path,
                input.health_interval.unwrap_or(30),
                60,
                input.field_mapping.as_ref().map(|m| serde_json::to_string(m).unwrap()),
                input.push_enabled.unwrap_or(false),
                now,
                now,
                true,
            ],
        )?;
        Ok(id)
    }

    pub fn get(&self, id: &str) -> rusqlite::Result<Option<Service>> {
        let conn = self.conn();
        let conn = conn.lock();
        conn.query_row(
            "SELECT id, name, kind, description, url, host, port,
                    process_label, script_path, health_mode, health_path,
                    health_interval, health_ttl, field_mapping, push_enabled,
                    registered_at, updated_at, enabled
             FROM services WHERE id = ?1",
            [id],
            row_to_service,
        ).optional()
    }
}

fn row_to_service(row: &rusqlite::Row) -> rusqlite::Result<Service> {
    let kind_str: String = row.get(2)?;
    let kind: ServiceKind = serde_json::from_str(&format!("\"{}\"", kind_str))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e)))?;
    let mode_str: String = row.get(9)?;
    let health_mode: HealthMode = serde_json::from_str(&format!("\"{}\"", mode_str))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e)))?;
    let field_mapping_str: Option<String> = row.get(13)?;
    let field_mapping = field_mapping_str.and_then(|s| serde_json::from_str(&s).ok());

    Ok(Service {
        id: row.get(0)?,
        name: row.get(1)?,
        kind,
        description: row.get(3)?,
        url: row.get(4)?,
        host: row.get(5)?,
        port: row.get::<_, Option<i64>>(6)?.map(|p| p as u16),
        process_label: row.get(7)?,
        script_path: row.get(8)?,
        health_mode,
        health_path: row.get(10)?,
        health_interval: row.get::<_, i64>(11)? as u32,
        health_ttl: row.get::<_, i64>(12)? as u32,
        field_mapping,
        push_enabled: row.get(14)?,
        registered_at: row.get(15)?,
        updated_at: row.get(16)?,
        enabled: row.get(17)?,
        health: HealthSnapshot::default(),
        schedule: None,
        next_run: None,
        last_run_status: None,
    })
}
```

- [ ] **Step 4: Run test, verify PASS**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib services::store::tests::create_and_get_daemon_service 2>&1 | tail -5
```
Expected: 1 passed.

- [ ] **Step 5: Add list, update, delete tests + implementations (one batch)**

Add to `tests.rs`:
```rust
#[test]
fn list_returns_all_services() {
    let store = ServicesStore::open_in_memory().unwrap();
    store.create(make_input("alpha")).unwrap();
    store.create(make_input("beta")).unwrap();
    let all = store.list(None, None).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn list_filters_by_kind() {
    let store = ServicesStore::open_in_memory().unwrap();
    store.create(make_input("daemon-svc")).unwrap();
    let daemons = store.list(Some(ServiceKind::Daemon), None).unwrap();
    assert_eq!(daemons.len(), 1);
    let crons = store.list(Some(ServiceKind::Cron), None).unwrap();
    assert_eq!(crons.len(), 0);
}

#[test]
fn update_changes_fields_and_bumps_updated_at() {
    let store = ServicesStore::open_in_memory().unwrap();
    let id = store.create(make_input("orig")).unwrap();
    let before = store.get(&id).unwrap().unwrap().updated_at;
    std::thread::sleep(std::time::Duration::from_millis(2));
    store.update(&id, ServiceUpdate {
        name: Some("renamed".into()),
        ..Default::default()
    }).unwrap();
    let after = store.get(&id).unwrap().unwrap();
    assert_eq!(after.name, "renamed");
    assert!(after.updated_at > before);
}

#[test]
fn delete_removes_service() {
    let store = ServicesStore::open_in_memory().unwrap();
    let id = store.create(make_input("doomed")).unwrap();
    store.delete(&id).unwrap();
    assert!(store.get(&id).unwrap().is_none());
}

fn make_input(name: &str) -> ServiceCreate {
    ServiceCreate {
        name: name.into(),
        kind: ServiceKind::Daemon,
        description: None,
        host: None,
        url: Some("http://127.0.0.1:0".into()),
        port: None,
        process_label: None,
        script_path: None,
        health_mode: HealthMode::Http,
        health_path: None,
        health_interval: None,
        field_mapping: None,
        push_enabled: None,
    }
}

impl Default for ServiceUpdate {
    fn default() -> Self {
        Self {
            name: None, description: None, url: None, host: None, port: None,
            process_label: None, script_path: None, health_mode: None, health_path: None,
            health_interval: None, field_mapping: None, push_enabled: None, enabled: None,
        }
    }
}
```

Add to `crud.rs`:
```rust
impl ServicesStore {
    pub fn list(&self, kind: Option<ServiceKind>, enabled: Option<bool>) -> rusqlite::Result<Vec<Service>> {
        let conn = self.conn();
        let conn = conn.lock();
        let mut sql = String::from(
            "SELECT id, name, kind, description, url, host, port,
                    process_label, script_path, health_mode, health_path,
                    health_interval, health_ttl, field_mapping, push_enabled,
                    registered_at, updated_at, enabled
             FROM services WHERE 1=1"
        );
        let mut clauses: Vec<String> = Vec::new();
        if let Some(k) = kind {
            clauses.push(format!(" AND kind = '{}'",
                serde_json::to_string(&k).unwrap().trim_matches('"')));
        }
        if let Some(e) = enabled {
            clauses.push(format!(" AND enabled = {}", if e { 1 } else { 0 }));
        }
        sql.push_str(&clauses.join(""));
        sql.push_str(" ORDER BY name");

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_service)?;
        rows.collect()
    }

    pub fn update(&self, id: &str, patch: ServiceUpdate) -> rusqlite::Result<()> {
        let now = now_ms();
        let conn = self.conn();
        let conn = conn.lock();
        if let Some(name) = patch.name {
            conn.execute("UPDATE services SET name=?1, updated_at=?2 WHERE id=?3", params![name, now, id])?;
        }
        if let Some(url) = patch.url {
            conn.execute("UPDATE services SET url=?1, updated_at=?2 WHERE id=?3", params![url, now, id])?;
        }
        if let Some(enabled) = patch.enabled {
            conn.execute("UPDATE services SET enabled=?1, updated_at=?2 WHERE id=?3", params![enabled, now, id])?;
        }
        // Other fields follow the same pattern; expand as needed.
        Ok(())
    }

    pub fn delete(&self, id: &str) -> rusqlite::Result<()> {
        let conn = self.conn();
        let conn = conn.lock();
        conn.execute("DELETE FROM services WHERE id=?1", [id])?;
        Ok(())
    }
}
```

- [ ] **Step 6: Run all store tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib services::store 2>&1 | tail -10
```
Expected: 5 passed.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/services/store/
git commit -m "feat(services): store CRUD (create/get/list/update/delete) with sqlite + tests"
```

### Task 1.3: Token store (file-based per-service tokens)

**Files:**
- Create: `src-tauri/src/services/tokens.rs`
- Modify: `src-tauri/src/services/mod.rs` (add `pub mod tokens;`)
- Test: `src-tauri/src/services/tokens.rs` (inline tests)

- [ ] **Step 1: Write failing test**

```rust
// in tokens.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generate_persist_validate_round_trip() {
        let dir = tempdir().unwrap();
        let store = TokenStore::new_with_root(dir.path().to_path_buf());
        let token = store.generate("svc_test").unwrap();
        assert_eq!(token.len(), 64); // 32 bytes hex
        assert!(store.validate("svc_test", &token).unwrap());
        assert!(!store.validate("svc_test", "wrong-token").unwrap());
    }

    #[test]
    fn missing_service_returns_none() {
        let dir = tempdir().unwrap();
        let store = TokenStore::new_with_root(dir.path().to_path_buf());
        assert!(!store.validate("svc_nonexistent", "any").unwrap());
    }

    #[test]
    fn rotate_invalidates_old_token() {
        let dir = tempdir().unwrap();
        let store = TokenStore::new_with_root(dir.path().to_path_buf());
        let old = store.generate("svc_x").unwrap();
        let new = store.generate("svc_x").unwrap();
        assert_ne!(old, new);
        assert!(!store.validate("svc_x", &old).unwrap());
        assert!(store.validate("svc_x", &new).unwrap());
    }

    #[test]
    fn lookup_service_id_by_token() {
        let dir = tempdir().unwrap();
        let store = TokenStore::new_with_root(dir.path().to_path_buf());
        let token_a = store.generate("svc_a").unwrap();
        let token_b = store.generate("svc_b").unwrap();
        assert_eq!(store.lookup_service_id(&token_a).unwrap(), Some("svc_a".to_string()));
        assert_eq!(store.lookup_service_id(&token_b).unwrap(), Some("svc_b".to_string()));
        assert_eq!(store.lookup_service_id("garbage").unwrap(), None);
    }
}
```

- [ ] **Step 2: Implement `tokens.rs`**

```rust
//! File-based per-service token store. Tokens live as
//! `<root>/<service_id>/token` (mode 0600). `lookup_service_id` linearly
//! scans the directory — fine for the expected count (typically <30 services)
//! but will need an in-memory index if scale ever grows.

use rand::RngCore;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub struct TokenStore {
    root: PathBuf,
}

impl TokenStore {
    pub fn new() -> Self {
        let root = crate::paths::data_dir().join("services");
        Self::new_with_root(root)
    }

    pub fn new_with_root(root: PathBuf) -> Self {
        let _ = fs::create_dir_all(&root);
        Self { root }
    }

    fn token_path(&self, service_id: &str) -> PathBuf {
        self.root.join(service_id).join("token")
    }

    pub fn generate(&self, service_id: &str) -> std::io::Result<String> {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let path = self.token_path(service_id);
        fs::create_dir_all(path.parent().unwrap())?;
        let mut f = fs::File::create(&path)?;
        f.write_all(token.as_bytes())?;
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        Ok(token)
    }

    pub fn validate(&self, service_id: &str, candidate: &str) -> std::io::Result<bool> {
        let path = self.token_path(service_id);
        if !path.exists() {
            return Ok(false);
        }
        let stored = fs::read_to_string(&path)?;
        Ok(constant_time_eq(stored.trim().as_bytes(), candidate.as_bytes()))
    }

    pub fn lookup_service_id(&self, token: &str) -> std::io::Result<Option<String>> {
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().to_string();
            if self.validate(&id, token)? {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    pub fn delete(&self, service_id: &str) -> std::io::Result<()> {
        let dir = self.root.join(service_id);
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
        Ok(())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) { diff |= x ^ y; }
    diff == 0
}
```

- [ ] **Step 3: Add `pub mod tokens;` in `services/mod.rs`**

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib services::tokens 2>&1 | tail -8
```
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/tokens.rs src-tauri/src/services/mod.rs
git commit -m "feat(services): file-based per-service token store with constant-time validation"
```

### Task 1.4: Cron-as-virtual-Service projection

**Files:**
- Create: `src-tauri/src/services/store/cron_view.rs`
- Modify: `src-tauri/src/services/store/mod.rs`
- Test: same file

- [ ] **Step 1: Write failing test**

```rust
// in tests.rs (add to existing file)
#[test]
fn list_local_includes_cron_jobs_as_virtual_services() {
    use crate::cron::store::{CronCreateParams, CronStore};
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let cron_path = dir.path().join("crons.json");
    let mut cron_store = CronStore::new_with_path(cron_path);
    cron_store.create(CronCreateParams {
        name: "test-cron".into(),
        workspace_id: "ws".into(),
        schedule: "0 * * * *".into(),
        task: "echo".into(),
        description: None, agent: None, model: None, cwd: None, role: None,
        creator_pane_id: None, enabled: Some(true), max_history: None,
        oneshot: None, jitter_max_ms: Some(0), tz: None, timeout_seconds: None,
    }).unwrap();

    let svc_store = ServicesStore::open_in_memory().unwrap();
    let unified = list_local(&svc_store, &cron_store).unwrap();

    let cron_svc = unified.iter().find(|s| s.name == "test-cron").expect("cron present as service");
    assert_eq!(cron_svc.kind, ServiceKind::Cron);
    assert_eq!(cron_svc.schedule.as_deref(), Some("0 * * * *"));
    assert!(cron_svc.next_run.is_some());
}
```

- [ ] **Step 2: Implement `cron_view.rs`**

```rust
//! Project CronJob into Service so the unified registry shows crons + daemons together.

use super::types::*;
use crate::cron::store::{CronJob, CronStore};

pub fn cron_to_service(job: &CronJob) -> Service {
    Service {
        id: format!("cron_{}", job.id),
        name: job.name.clone(),
        kind: ServiceKind::Cron,
        description: job.description.clone(),
        host: None,
        url: None,
        port: None,
        process_label: None,
        script_path: None,
        health_mode: HealthMode::Http,  // unused for cron
        health_path: None,
        health_interval: 0,
        health_ttl: 0,
        field_mapping: None,
        push_enabled: false,
        schedule: Some(job.schedule.clone()),
        next_run: job.next_run.map(|n| n as i64),
        last_run_status: job.last_run.as_ref().map(|r| format!("{:?}", r.status)),
        registered_at: job.created_at as i64,
        updated_at: job.created_at as i64,
        enabled: job.enabled,
        health: HealthSnapshot::default(),
    }
}

pub fn list_local(svc: &super::ServicesStore, cron: &CronStore) -> rusqlite::Result<Vec<Service>> {
    let mut out = svc.list(None, None)?;
    out.extend(cron.list().iter().map(cron_to_service));
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
```

- [ ] **Step 3: Wire `mod cron_view;` and `pub use cron_view::list_local;` in `services/store/mod.rs`**

- [ ] **Step 4: Run test**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib services::store::tests::list_local_includes_cron 2>&1 | tail -5
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/store/
git commit -m "feat(services): project cron jobs as virtual Service entries (kind=cron)"
```

---

## Phase 2: Backend services REST + Tauri commands

### Task 2.1: REST routes (axum)

**Files:**
- Create: `src-tauri/src/api/services_routes.rs`
- Modify: `src-tauri/src/api/routes.rs` (register routes + AppState extension)

- [ ] **Step 1: Write integration test for `GET /api/services`**

Add to `src-tauri/src/api/services_routes.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn get_services_returns_empty_when_none_registered() {
        let state = test_state();
        let app = router().with_state(state);
        let resp = app.oneshot(
            Request::builder().uri("/api/services").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), 200);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
        assert!(arr.is_empty());
    }

    fn test_state() -> AppState { /* construct minimal AppState — see implementation */ }
}
```

- [ ] **Step 2: Implement `services_routes.rs`**

```rust
//! REST endpoints for services::store + activity log.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::services::store::{Service, ServiceCreate, ServiceUpdate, ServiceKind};

#[derive(Deserialize)]
pub struct ListQuery {
    pub kind: Option<String>,
    pub enabled: Option<bool>,
}

pub async fn list_services(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<Service>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let kind = q.kind.and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok());
    let services = crate::services::store::list_local(&state.svc.services_store, &state.svc.cron.lock())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let filtered: Vec<Service> = services.into_iter()
        .filter(|s| kind.as_ref().map_or(true, |k: &ServiceKind| &s.kind == k))
        .filter(|s| q.enabled.map_or(true, |e| s.enabled == e))
        .collect();
    Ok(Json(filtered))
}

pub async fn create_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ServiceCreate>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if body.kind != ServiceKind::Daemon {
        return Err((StatusCode::BAD_REQUEST, "Only daemon-kind services can be created via this endpoint; use /api/crons for cron jobs".into()));
    }
    let id = state.svc.services_store.create(body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let token = state.svc.tokens.generate(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let service = state.svc.services_store.get(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "service vanished after create".into()))?;
    Ok(Json(serde_json::json!({"service": service, "token": token})))
}

pub async fn get_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Service>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state.svc.services_store.get(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, format!("service {} not found", id)))
}

pub async fn update_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ServiceUpdate>,
) -> Result<Json<Service>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state.svc.services_store.update(&id, body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state.svc.services_store.get(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, format!("service {} not found after update", id)))
}

pub async fn delete_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state.svc.services_store.delete(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state.svc.tokens.delete(&id).ok();
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn regenerate_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let token = state.svc.tokens.generate(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"token": token})))
}
```

- [ ] **Step 3: Register routes in `api/routes.rs`**

Find the existing route registration block and add:
```rust
.route("/api/services", get(services_routes::list_services).post(services_routes::create_service))
.route("/api/services/{id}", get(services_routes::get_service).put(services_routes::update_service).delete(services_routes::delete_service))
.route("/api/services/{id}/regenerate-token", post(services_routes::regenerate_token))
```

Add `services_store: Arc<ServicesStore>` and `tokens: Arc<TokenStore>` to the `Service` struct (in `src-tauri/src/service/mod.rs`); initialize in startup with `crate::paths::data_dir().join("services.db")` and `TokenStore::new()` respectively.

- [ ] **Step 4: Run cargo check + REST integration tests**

```bash
cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tail -5
cargo test --manifest-path src-tauri/Cargo.toml --lib api::services_routes 2>&1 | tail -10
```
Expected: 0 compile errors. REST tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/api/services_routes.rs src-tauri/src/api/routes.rs src-tauri/src/service/
git commit -m "feat(services): REST API (CRUD + token regeneration) wired to AppState"
```

### Task 2.2: Tauri commands + JSON-RPC dispatch

**Files:**
- Create: `src-tauri/src/commands/services.rs`
- Create: `src-tauri/src/socket/server/services.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/socket/server/mod.rs`
- Modify: `src-tauri/src/lib.rs` (register commands in tauri::Builder)

- [ ] **Step 1: Implement Tauri commands**

```rust
// src-tauri/src/commands/services.rs
use crate::services::store::{Service, ServiceCreate, ServiceKind, ServiceUpdate};

#[tauri::command]
pub fn services_list(
    svc: tauri::State<'_, crate::service::ServiceState>,
    kind: Option<String>,
    enabled: Option<bool>,
) -> Result<Vec<Service>, String> {
    let kind: Option<ServiceKind> = kind.and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok());
    let cron = svc.cron.lock();
    let mut services = crate::services::store::list_local(&svc.services_store, &cron)
        .map_err(|e| e.to_string())?;
    services.retain(|s| kind.as_ref().map_or(true, |k| &s.kind == k));
    services.retain(|s| enabled.map_or(true, |e| s.enabled == e));
    Ok(services)
}

#[tauri::command]
pub fn services_create(
    svc: tauri::State<'_, crate::service::ServiceState>,
    input: ServiceCreate,
) -> Result<serde_json::Value, String> {
    let id = svc.services_store.create(input).map_err(|e| e.to_string())?;
    let token = svc.tokens.generate(&id).map_err(|e| e.to_string())?;
    let service = svc.services_store.get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "service vanished after create".to_string())?;
    Ok(serde_json::json!({"service": service, "token": token}))
}

#[tauri::command]
pub fn services_update(
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
    patch: ServiceUpdate,
) -> Result<Service, String> {
    svc.services_store.update(&id, patch).map_err(|e| e.to_string())?;
    svc.services_store.get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "service not found after update".to_string())
}

#[tauri::command]
pub fn services_delete(
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<(), String> {
    svc.services_store.delete(&id).map_err(|e| e.to_string())?;
    svc.tokens.delete(&id).ok();
    Ok(())
}

#[tauri::command]
pub fn services_regenerate_token(
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<String, String> {
    svc.tokens.generate(&id).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Implement JSON-RPC dispatch**

```rust
// src-tauri/src/socket/server/services.rs
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::service::ServiceState;
use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    _app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "services.list" => {
            let cron = svc.cron.lock();
            let services = crate::services::store::list_local(&svc.services_store, &cron)
                .map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })?;
            serde_json::to_value(&services)
                .map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }
        "services.create" => {
            let p = params.clone().ok_or_else(|| JsonRpcError { code: -32602, message: "params required".into() })?;
            let input: crate::services::store::ServiceCreate = serde_json::from_value(p)
                .map_err(|e| JsonRpcError { code: -32602, message: format!("Invalid params: {}", e) })?;
            let id = svc.services_store.create(input)
                .map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })?;
            let token = svc.tokens.generate(&id)
                .map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })?;
            let service = svc.services_store.get(&id)
                .map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })?
                .ok_or_else(|| JsonRpcError { code: -32603, message: "service vanished".into() })?;
            Ok(serde_json::json!({"service": service, "token": token}))
        }
        "services.delete" => {
            let id = get_string_param(params, "id")?;
            svc.services_store.delete(&id)
                .map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })?;
            svc.tokens.delete(&id).ok();
            Ok(serde_json::json!({"ok": true}))
        }
        _ => Err(JsonRpcError { code: -32601, message: "Method not found".to_string() }),
    }
}
```

- [ ] **Step 3: Register dispatch + commands**

In `src-tauri/src/socket/server/mod.rs` find the dispatch fan-out (where `cron::dispatch` is called) and add:
```rust
"services.list" | "services.create" | "services.delete" => services::dispatch(method, params, &app, svc),
```

Also add to the method-list (used for help text): `"services.list", "services.create", "services.delete"`.

In `src-tauri/src/lib.rs` find the `tauri::Builder::default().invoke_handler(...)` block and add command registrations:
```rust
crate::commands::services::services_list,
crate::commands::services::services_create,
crate::commands::services::services_update,
crate::commands::services::services_delete,
crate::commands::services::services_regenerate_token,
```

- [ ] **Step 4: cargo check**

```bash
cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tail -5
```
Expected: 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/services.rs src-tauri/src/commands/mod.rs src-tauri/src/socket/server/services.rs src-tauri/src/socket/server/mod.rs src-tauri/src/lib.rs
git commit -m "feat(services): Tauri commands + JSON-RPC dispatch (services.list/create/delete)"
```

### Task 2.3: CLI `codeterm service *` commands

**Files:**
- Create: `cli/src/args/service.rs`
- Create: `cli/src/cmd/service.rs`
- Modify: `cli/src/args/mod.rs`, `cli/src/args/cli.rs`, `cli/src/cmd/mod.rs`, `cli/src/main.rs`

- [ ] **Step 1: Define CLI args**

```rust
// cli/src/args/service.rs
use clap::Subcommand;

#[derive(Subcommand)]
pub enum ServiceAction {
    /// List all registered services (cron + daemon)
    List {
        #[arg(long)] kind: Option<String>,
    },
    /// Register a new daemon-kind service
    Add {
        name: String,
        #[arg(long, default_value = "daemon")] kind: String,
        #[arg(long, default_value = "http")] health_mode: String,
        #[arg(long)] url: Option<String>,
        #[arg(long)] host: Option<String>,
        #[arg(long)] port: Option<u16>,
        #[arg(long)] process_label: Option<String>,
        #[arg(long)] script_path: Option<String>,
        #[arg(long)] health_path: Option<String>,
        #[arg(long, default_value = "30")] health_interval: u32,
        #[arg(long)] description: Option<String>,
        #[arg(long, default_value = "false")] push: bool,
    },
    /// Show service detail
    Show { id: String },
    /// Edit a service
    Edit {
        id: String,
        #[arg(long)] name: Option<String>,
        #[arg(long)] url: Option<String>,
        #[arg(long)] enabled: Option<bool>,
    },
    /// Delete a service
    Delete { id: String },
    /// Force a health probe
    Probe { id: String },
    /// Show or rotate the push token
    Token {
        id: String,
        #[arg(long)] rotate: bool,
    },
}
```

- [ ] **Step 2: Implement command dispatch**

```rust
// cli/src/cmd/service.rs
use crate::client::SocketClient;
use crate::ServiceAction;

pub fn handle_service(client: &mut SocketClient, json: bool, action: ServiceAction) {
    match action {
        ServiceAction::List { kind } => {
            let mut params = serde_json::json!({});
            if let Some(k) = kind { params["kind"] = serde_json::json!(k); }
            match client.call("services.list", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() { println!("No services registered."); }
                        for s in arr {
                            println!("{} {} [{}] kind={} url={}",
                                s.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                                s.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                                s.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                                s.get("kind").and_then(|v| v.as_str()).unwrap_or("?"),
                                s.get("url").and_then(|v| v.as_str()).unwrap_or("-"));
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        ServiceAction::Add { name, kind, health_mode, url, host, port, process_label, script_path, health_path, health_interval, description, push } => {
            let params = serde_json::json!({
                "name": name, "kind": kind, "health_mode": health_mode,
                "url": url, "host": host, "port": port,
                "process_label": process_label, "script_path": script_path,
                "health_path": health_path, "health_interval": health_interval,
                "description": description, "push_enabled": push,
            });
            match client.call("services.create", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        let svc = val.get("service").unwrap();
                        let token = val.get("token").and_then(|v| v.as_str()).unwrap_or("");
                        println!("Created: {} ({})",
                            svc.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                            svc.get("id").and_then(|v| v.as_str()).unwrap_or("?"));
                        if push {
                            println!("Push token (save this NOW, will not be shown again):");
                            println!("  {}", token);
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        ServiceAction::Delete { id } => {
            match client.call("services.delete", Some(serde_json::json!({"id": id}))) {
                Ok(_) => println!("Deleted: {}", id),
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        _ => {
            eprintln!("Action not yet implemented in CLI");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 3: Wire into main.rs/mod.rs/cli.rs alongside `cron`/`mem` patterns**

In `cli/src/args/cli.rs` add `Service { #[command(subcommand)] action: ServiceAction }` to the Commands enum.

In `cli/src/cmd/mod.rs` add `pub mod service;` and route in main.rs `Commands::Service { action } => cmd::service::handle_service(&mut client, json, action),`.

- [ ] **Step 4: Build CLI and smoke-test**

```bash
cargo build --manifest-path cli/Cargo.toml 2>&1 | tail -3
./target/debug/codeterm service --help
./target/debug/codeterm service list
```
Expected: help displays all subcommands; list returns empty/error if dev not running.

- [ ] **Step 5: Commit**

```bash
git add cli/src/
git commit -m "feat(services): CLI codeterm service add/list/show/edit/delete/probe/token"
```

---

## Phase 3: Backend push-token middleware

### Task 3.1: Bearer-token middleware on `/api/panes/:id/send`

**Files:**
- Modify: `src-tauri/src/api/pane_routes.rs` (or wherever the send handler lives)
- Modify: `src-tauri/src/services/push.rs` (NEW)
- Test: same

- [ ] **Step 1: Write integration test**

```rust
// src-tauri/src/services/push.rs (test module)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_service_token_extracts_service_id() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = crate::services::tokens::TokenStore::new_with_root(dir.path().to_path_buf());
        let token = tokens.generate("svc_test").unwrap();
        let auth_header = format!("Bearer {}", token);
        let result = extract_service_token(&auth_header, &tokens).unwrap();
        assert_eq!(result, Some("svc_test".to_string()));
    }

    #[test]
    fn missing_bearer_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = crate::services::tokens::TokenStore::new_with_root(dir.path().to_path_buf());
        let result = extract_service_token("Bearer not-a-real-token", &tokens).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn malformed_header_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = crate::services::tokens::TokenStore::new_with_root(dir.path().to_path_buf());
        assert_eq!(extract_service_token("not-bearer xxx", &tokens).unwrap(), None);
        assert_eq!(extract_service_token("", &tokens).unwrap(), None);
    }
}
```

- [ ] **Step 2: Implement `push.rs`**

```rust
//! Bearer-token middleware for /api/panes/:id/send extension.
//!
//! When `Authorization: Bearer <tk>` is present:
//!   - Look up <tk> in TokenStore
//!   - If found AND service is enabled AND push_enabled → return Some(service_id)
//!   - Otherwise → None (caller falls through to existing API token auth)
//!
//! Service tokens are NOT valid for any other endpoint. They authorize ONE
//! thing: pane-write. Scope discipline.

use crate::services::store::ServicesStore;
use crate::services::tokens::TokenStore;

pub fn extract_service_token(
    auth_header: &str,
    tokens: &TokenStore,
) -> std::io::Result<Option<String>> {
    let prefix = "Bearer ";
    if !auth_header.starts_with(prefix) {
        return Ok(None);
    }
    let token = &auth_header[prefix.len()..];
    if token.is_empty() {
        return Ok(None);
    }
    tokens.lookup_service_id(token)
}

pub fn validate_push(
    auth_header: &str,
    tokens: &TokenStore,
    store: &ServicesStore,
) -> std::io::Result<Option<String>> {
    let Some(svc_id) = extract_service_token(auth_header, tokens)? else {
        return Ok(None);
    };
    let Some(service) = store.get(&svc_id).ok().flatten() else {
        return Ok(None);
    };
    if service.enabled && service.push_enabled {
        Ok(Some(svc_id))
    } else {
        Ok(None)
    }
}
```

- [ ] **Step 3: Modify `pane_routes.rs` send handler to consult validate_push**

Find the existing `send_text` handler. Before its current auth check, add:
```rust
let svc_token_match = headers.get("Authorization")
    .and_then(|v| v.to_str().ok())
    .and_then(|v| crate::services::push::validate_push(v, &state.svc.tokens, &state.svc.services_store).ok().flatten());

if svc_token_match.is_none() {
    // Fall through to existing API token auth (preserves cron/agent flow)
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
}
// else: service-token authorized; svc_token_match holds the service_id.
// Stamp it on the activity log when we record the push event in Phase 4.
```

- [ ] **Step 4: cargo check + run push tests**

```bash
cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tail -5
cargo test --manifest-path src-tauri/Cargo.toml --lib services::push 2>&1 | tail -8
```
Expected: 3 push tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/push.rs src-tauri/src/services/mod.rs src-tauri/src/api/pane_routes.rs
git commit -m "feat(services): Bearer-token middleware on /api/panes/:id/send (services::push)"
```

### Task 3.2: Activity log writer + reader

**Files:**
- Create: `src-tauri/src/services/activity.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Test: same file

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::ServicesStore;

    #[test]
    fn record_push_event_persists_with_source_metadata() {
        let store = ServicesStore::open_in_memory().unwrap();
        let activity = ActivityWriter::new(store.conn());
        activity.record(ActivityEvent::push("svc_voice", "pane_a3f1", "transcript: hello")).unwrap();
        let events = activity.list(None, None, Some(10)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].source, ActivitySource::Push);
        assert_eq!(events[0].source_id.as_deref(), Some("svc_voice"));
        assert_eq!(events[0].target.as_deref(), Some("pane_a3f1"));
    }

    #[test]
    fn eviction_keeps_recent_drops_old() {
        let store = ServicesStore::open_in_memory().unwrap();
        let activity = ActivityWriter::new(store.conn());
        // Insert 10003 events, expect bounded to 10000 after eviction
        for i in 0..10003 {
            activity.record(ActivityEvent::peer("svc", "msg", &format!("e{}", i))).unwrap();
        }
        activity.evict_if_needed().unwrap();
        let count = activity.count().unwrap();
        assert!(count <= 10000, "got {}", count);
    }

    #[test]
    fn list_filters_by_source() {
        let store = ServicesStore::open_in_memory().unwrap();
        let activity = ActivityWriter::new(store.conn());
        activity.record(ActivityEvent::push("svc1", "pane1", "x")).unwrap();
        activity.record(ActivityEvent::peer("peer1", "info", "msg")).unwrap();
        let pushes = activity.list(Some(ActivitySource::Push), None, None).unwrap();
        assert_eq!(pushes.len(), 1);
    }
}
```

- [ ] **Step 2: Implement `activity.rs`**

```rust
//! Bounded activity log for the Mesh tab.
//!
//! Eviction policy: keep last 10000 rows OR drop rows older than 30 days,
//! whichever evicts more. Run on every 100th insert (cheap heuristic, no timer).

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_ROWS: usize = 10_000;
const MAX_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
const EVICT_EVERY: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivitySource {
    Push,
    Health,
    Peer,
    Cron,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityLevel { Info, Warn, Error }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: Option<i64>,
    pub occurred_at: i64,
    pub source: ActivitySource,
    pub source_id: Option<String>,
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_peer: Option<String>,
    pub level: ActivityLevel,
    pub message: String,
    pub meta: serde_json::Value,
}

impl ActivityEvent {
    pub fn push(service_id: &str, pane_id: &str, body_preview: &str) -> Self {
        Self {
            id: None,
            occurred_at: now_ms(),
            source: ActivitySource::Push,
            source_id: Some(service_id.into()),
            target: Some(pane_id.into()),
            source_peer: None,
            level: ActivityLevel::Info,
            message: format!("{} → pane {}: {}", service_id, pane_id, &body_preview.chars().take(80).collect::<String>()),
            meta: serde_json::json!({"body_preview": body_preview}),
        }
    }

    pub fn peer(source_id: &str, level_str: &str, message: &str) -> Self {
        let level = match level_str {
            "warn" => ActivityLevel::Warn,
            "error" => ActivityLevel::Error,
            _ => ActivityLevel::Info,
        };
        Self {
            id: None,
            occurred_at: now_ms(),
            source: ActivitySource::Peer,
            source_id: Some(source_id.into()),
            target: None,
            source_peer: None,
            level,
            message: message.into(),
            meta: serde_json::json!({}),
        }
    }

    pub fn health(service_id: &str, transition: (&str, &str)) -> Self {
        Self {
            id: None,
            occurred_at: now_ms(),
            source: ActivitySource::Health,
            source_id: Some(service_id.into()),
            target: None,
            source_peer: None,
            level: if transition.1 == "down" { ActivityLevel::Error } else { ActivityLevel::Info },
            message: format!("{}: {} → {}", service_id, transition.0, transition.1),
            meta: serde_json::json!({"from": transition.0, "to": transition.1}),
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

pub struct ActivityWriter {
    conn: Arc<Mutex<Connection>>,
    insert_count: Arc<Mutex<usize>>,
}

impl ActivityWriter {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn, insert_count: Arc::new(Mutex::new(0)) }
    }

    pub fn record(&self, ev: ActivityEvent) -> rusqlite::Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO activity (occurred_at, source, source_id, target, level, message, meta)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                ev.occurred_at,
                serde_json::to_string(&ev.source).unwrap().trim_matches('"').to_string(),
                ev.source_id,
                ev.target,
                serde_json::to_string(&ev.level).unwrap().trim_matches('"').to_string(),
                ev.message,
                ev.meta.to_string(),
            ],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);

        let mut counter = self.insert_count.lock();
        *counter += 1;
        if *counter >= EVICT_EVERY {
            *counter = 0;
            drop(counter);
            self.evict_if_needed().ok();
        }
        Ok(id)
    }

    pub fn evict_if_needed(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        let cutoff = now_ms() - MAX_AGE_MS;
        conn.execute("DELETE FROM activity WHERE occurred_at < ?1", [cutoff])?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM activity", [], |r| r.get(0))?;
        if count > MAX_ROWS as i64 {
            let to_drop = count - MAX_ROWS as i64;
            conn.execute(
                "DELETE FROM activity WHERE id IN (SELECT id FROM activity ORDER BY occurred_at ASC LIMIT ?1)",
                [to_drop],
            )?;
        }
        Ok(())
    }

    pub fn list(
        &self,
        source: Option<ActivitySource>,
        since: Option<i64>,
        limit: Option<usize>,
    ) -> rusqlite::Result<Vec<ActivityEvent>> {
        let conn = self.conn.lock();
        let mut sql = String::from("SELECT id, occurred_at, source, source_id, target, level, message, meta FROM activity WHERE 1=1");
        if source.is_some() { sql.push_str(" AND source = ?1"); }
        if since.is_some() { sql.push_str(if source.is_some() { " AND occurred_at > ?2" } else { " AND occurred_at > ?1" }); }
        sql.push_str(" ORDER BY occurred_at DESC LIMIT ?");
        sql.push_str(&format!("{}", if source.is_some() && since.is_some() { 3 } else if source.is_some() || since.is_some() { 2 } else { 1 }));

        let row_to_event = |row: &rusqlite::Row| -> rusqlite::Result<ActivityEvent> {
            let source_str: String = row.get(2)?;
            let source = serde_json::from_str(&format!("\"{}\"", source_str))
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e)))?;
            let level_str: String = row.get(5)?;
            let level = serde_json::from_str(&format!("\"{}\"", level_str))
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?;
            let meta_str: Option<String> = row.get(7)?;
            let meta = meta_str.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({}));
            Ok(ActivityEvent {
                id: Some(row.get(0)?),
                occurred_at: row.get(1)?,
                source,
                source_id: row.get(3)?,
                target: row.get(4)?,
                source_peer: None,
                level,
                message: row.get(6)?,
                meta,
            })
        };

        let limit_val = limit.unwrap_or(500) as i64;
        let mut stmt = conn.prepare(&sql)?;
        match (source, since) {
            (Some(s), Some(t)) => {
                let s_str = serde_json::to_string(&s).unwrap().trim_matches('"').to_string();
                let rows = stmt.query_map(params![s_str, t, limit_val], row_to_event)?;
                rows.collect()
            }
            (Some(s), None) => {
                let s_str = serde_json::to_string(&s).unwrap().trim_matches('"').to_string();
                let rows = stmt.query_map(params![s_str, limit_val], row_to_event)?;
                rows.collect()
            }
            (None, Some(t)) => {
                let rows = stmt.query_map(params![t, limit_val], row_to_event)?;
                rows.collect()
            }
            (None, None) => {
                let rows = stmt.query_map([limit_val], row_to_event)?;
                rows.collect()
            }
        }
    }

    pub fn count(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM activity", [], |r| r.get(0))
    }
}
```

- [ ] **Step 3: Wire `pub mod activity;` in `services/mod.rs`**

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib services::activity 2>&1 | tail -8
```
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/activity.rs src-tauri/src/services/mod.rs
git commit -m "feat(services): bounded activity log writer + reader (10k rows / 30 days)"
```

### Task 3.3: Wire activity-write into push handler

**Files:**
- Modify: `src-tauri/src/api/pane_routes.rs`

- [ ] **Step 1: Locate the send-text handler** (where Phase 3 Task 3.1 added validate_push)

- [ ] **Step 2: After successful pane write, record activity event**

```rust
// In the success branch of the send handler, after pane.send_text succeeds:
if let Some(svc_id) = svc_token_match {
    let writer = crate::services::activity::ActivityWriter::new(state.svc.services_store.conn());
    let _ = writer.record(crate::services::activity::ActivityEvent::push(
        &svc_id,
        &pane_id,
        &body.text.chars().take(200).collect::<String>(),
    ));
}
```

- [ ] **Step 3: Verify cargo check**

```bash
cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tail -5
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/api/pane_routes.rs
git commit -m "feat(services): record push events into activity log on successful pane write"
```

---

## Phase 4: Backend health probe scheduler

### Task 4.1: HTTP health mode

**Files:**
- Create: `src-tauri/src/services/health/mod.rs`
- Create: `src-tauri/src/services/health/http.rs`
- Modify: `src-tauri/src/services/mod.rs`

- [ ] **Step 1: Tests for HTTP probe (200 ok, 500 down, timeout, JSON parse)**

```rust
// in services/health/http.rs (test module)
#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn http_probe_200_ok_with_json() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/healthz")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ok","version":"1.2","uptime_secs":3600}"#)
            .create_async().await;

        let snap = probe_http(&format!("{}{}", server.url(), "/healthz"), 5000).await.unwrap();
        assert_eq!(snap.status, crate::services::store::HealthStatus::Healthy);
        assert_eq!(snap.version.as_deref(), Some("1.2"));
        assert_eq!(snap.uptime_secs, Some(3600));
    }

    #[tokio::test]
    async fn http_probe_500_is_down() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/healthz").with_status(500).create_async().await;
        let snap = probe_http(&format!("{}/healthz", server.url()), 5000).await.unwrap();
        assert_eq!(snap.status, crate::services::store::HealthStatus::Down);
    }

    #[tokio::test]
    async fn http_probe_timeout_is_down() {
        let snap = probe_http("http://192.0.2.1:1/healthz", 100).await.unwrap();
        assert_eq!(snap.status, crate::services::store::HealthStatus::Down);
    }
}
```

- [ ] **Step 2: Implement `http.rs`**

```rust
//! HTTP health probe.

use std::time::{Duration, Instant};
use crate::services::store::{HealthSnapshot, HealthStatus};

pub async fn probe_http(url: &str, timeout_ms: u64) -> std::io::Result<HealthSnapshot> {
    let started = Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    match client.get(url).send().await {
        Ok(resp) => {
            let status_code = resp.status();
            let latency_ms = started.elapsed().as_millis() as u64;
            if !status_code.is_success() {
                return Ok(HealthSnapshot {
                    status: HealthStatus::Down,
                    probed_at: Some(now_ms()),
                    latency_ms: Some(latency_ms),
                    ..Default::default()
                });
            }
            let body = resp.text().await.unwrap_or_default();
            let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::json!({}));
            let status_str = parsed.get("status").and_then(|v| v.as_str()).unwrap_or("ok");
            let status = match status_str {
                "ok" | "healthy" => HealthStatus::Healthy,
                "degraded" => HealthStatus::Degraded,
                _ => HealthStatus::Down,
            };
            Ok(HealthSnapshot {
                status,
                probed_at: Some(now_ms()),
                latency_ms: Some(latency_ms),
                version: parsed.get("version").and_then(|v| v.as_str()).map(String::from),
                uptime_secs: parsed.get("uptime_secs").and_then(|v| v.as_u64()),
                extra: parsed,
            })
        }
        Err(_) => Ok(HealthSnapshot {
            status: HealthStatus::Down,
            probed_at: Some(now_ms()),
            latency_ms: Some(started.elapsed().as_millis() as u64),
            ..Default::default()
        }),
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}
```

- [ ] **Step 3: Add `mockito` to dev-dependencies in Cargo.toml**

```toml
[dev-dependencies]
mockito = "1"
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib services::health::http 2>&1 | tail -8
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/services/health/
git commit -m "feat(services): HTTP health probe with JSON parsing"
```

### Task 4.2-4.4: TCP, Process, Script health modes (one task each, similar pattern)

For each, follow the Task 4.1 pattern:
- Tests describing happy + sad paths
- Implementation using std-only primitives where possible (TcpStream::connect with timeout for tcp; Command::new("launchctl"/"systemctl") for process; Command::new(script_path) for script)
- Per-OS guards (cfg(target_os = "macos"/"linux"/"windows")) for process mode
- Commit each separately

(Pattern is mechanical — each module ~100-150 lines including tests. See spec §9.1 for test matrix.)

### Task 4.5: Background scheduler tokio task

**Files:**
- Create: `src-tauri/src/services/health/scheduler.rs`
- Modify: `src-tauri/src/services/health/mod.rs`
- Modify: `src-tauri/src/lib.rs` (start scheduler on app startup)

- [ ] **Step 1: Implement scheduler**

```rust
//! Background health probe scheduler — single tokio task that probes each
//! enabled service at its configured interval.

use std::sync::Arc;
use std::time::Duration;

use crate::services::store::{ServicesStore, HealthMode};
use crate::services::activity::{ActivityWriter, ActivityEvent};

pub fn start(
    store: Arc<ServicesStore>,
    activity: ActivityWriter,
    app_handle: tauri::AppHandle,
) {
    tokio::spawn(async move {
        log::info!("[services] health scheduler started");
        loop {
            let services = match store.list(None, Some(true)) {
                Ok(s) => s,
                Err(e) => { log::error!("[services] list failed: {}", e); tokio::time::sleep(Duration::from_secs(30)).await; continue; }
            };

            for svc in services {
                if svc.kind == crate::services::store::ServiceKind::Cron { continue; }

                let prev_status = svc.health.status;
                let snap = match svc.health_mode {
                    HealthMode::Http => {
                        let url = format!("{}{}", svc.url.unwrap_or_default(), svc.health_path.as_deref().unwrap_or("/healthz"));
                        super::http::probe_http(&url, 5000).await.unwrap_or_default()
                    }
                    HealthMode::Tcp => super::tcp::probe_tcp(&svc.host.unwrap_or_default(), svc.port.unwrap_or(0), 3000).await.unwrap_or_default(),
                    HealthMode::Process => super::process::probe_process(&svc.process_label.unwrap_or_default()).unwrap_or_default(),
                    HealthMode::Script => super::script::probe_script(&svc.script_path.unwrap_or_default(), 30_000).await.unwrap_or_default(),
                };

                // Persist snapshot to service_health (limit retention separately)
                // ...

                // Emit transition event
                if snap.status != prev_status {
                    let _ = activity.record(ActivityEvent::health(
                        &svc.id,
                        (&format!("{:?}", prev_status).to_lowercase(), &format!("{:?}", snap.status).to_lowercase()),
                    ));
                    let _ = tauri::Emitter::emit(&app_handle, "service-health-changed",
                        serde_json::json!({"service_id": svc.id, "status": snap.status}));
                }
            }

            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
```

- [ ] **Step 2: Wire startup in `lib.rs`**

In the existing `tauri::Builder::default().setup(|app| { ... })` block, after Mesh/Services setup:
```rust
let store = service_state.services_store.clone();
let activity = crate::services::activity::ActivityWriter::new(store.conn());
crate::services::health::scheduler::start(store, activity, app.handle().clone());
```

- [ ] **Step 3: Cargo check + restart dev**

```bash
cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tail -3
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/health/ src-tauri/src/lib.rs
git commit -m "feat(services): tokio scheduler probes enabled services every 30s, emits transitions"
```

---

## Phase 5: Frontend MeshPanel skeleton + sub-tab router

### Task 5.1: Create MeshPanel root component

**Files:**
- Create: `src/components/MeshPanel/MeshPanel.tsx`
- Create: `src/components/MeshPanel/types.ts`
- Create: `src/components/MeshPanel/meshApi.ts`

- [ ] **Step 1: Create types mirroring Rust structs**

```typescript
// src/components/MeshPanel/types.ts
export type ServiceKind = "cron" | "daemon";
export type HealthMode = "http" | "tcp" | "process" | "script";
export type HealthStatus = "healthy" | "degraded" | "down" | "unknown";
export type ActivitySource = "push" | "health" | "peer" | "cron";
export type ActivityLevel = "info" | "warn" | "error";

export interface HealthSnapshot {
  status: HealthStatus;
  probed_at?: number;
  latency_ms?: number;
  version?: string;
  uptime_secs?: number;
  extra?: Record<string, unknown>;
}

export interface Service {
  id: string;
  name: string;
  kind: ServiceKind;
  description?: string;
  host?: string;
  url?: string;
  port?: number;
  process_label?: string;
  script_path?: string;
  health_mode: HealthMode;
  health_path?: string;
  health_interval: number;
  health: HealthSnapshot;
  push_enabled: boolean;
  schedule?: string;
  next_run?: number;
  last_run_status?: string;
  registered_at: number;
  updated_at: number;
  enabled: boolean;
  source_peer?: string; // populated client-side when merging from remote peers
}

export interface ActivityEvent {
  id?: number;
  occurred_at: number;
  source: ActivitySource;
  source_id?: string;
  target?: string;
  source_peer?: string;
  level: ActivityLevel;
  message: string;
  meta: Record<string, unknown>;
}

export type MeshTab = "map" | "activity" | "settings";
```

- [ ] **Step 2: Create thin invoke wrapper**

```typescript
// src/components/MeshPanel/meshApi.ts
import { invoke } from "@tauri-apps/api/core";
import type { Service, ActivityEvent, ServiceKind } from "./types";

export async function servicesList(filter?: { kind?: ServiceKind; enabled?: boolean }): Promise<Service[]> {
  return invoke("services_list", filter ?? {});
}

export async function servicesCreate(input: Partial<Service>): Promise<{ service: Service; token: string }> {
  return invoke("services_create", { input });
}

export async function servicesUpdate(id: string, patch: Partial<Service>): Promise<Service> {
  return invoke("services_update", { id, patch });
}

export async function servicesDelete(id: string): Promise<void> {
  return invoke("services_delete", { id });
}

export async function servicesRegenerateToken(id: string): Promise<string> {
  return invoke("services_regenerate_token", { id });
}

export async function activityList(opts?: { source?: string; since?: number; limit?: number }): Promise<ActivityEvent[]> {
  return invoke("activity_list", opts ?? {});
}
```

- [ ] **Step 3: Create MeshPanel root component (mirrors MemoryPanel layout exactly)**

```typescript
// src/components/MeshPanel/MeshPanel.tsx
import { useEffect, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import type { MeshTab } from "./types";

const TAB_BASE = "px-3 py-2 text-xs border-b-2 bg-transparent cursor-pointer capitalize";
const TAB_ACTIVE = `${TAB_BASE} border-[var(--ct-accent)] text-[var(--ct-fg)]`;
const TAB_INACTIVE = `${TAB_BASE} border-transparent text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]`;

const TABS: MeshTab[] = ["map", "activity", "settings"];

interface Props {
  api: ApiConfig;
  onClose: () => void;
}

export function MeshPanel({ api, onClose }: Props) {
  const [activeTab, setActiveTab] = useState<MeshTab>("map");

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const inField = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (e.key === "Escape" && !inField) { e.preventDefault(); onClose(); return; }
      if (inField) return;
      if (e.key === "1") { e.preventDefault(); setActiveTab("map"); }
      else if (e.key === "2") { e.preventDefault(); setActiveTab("activity"); }
      else if (e.key === "3") { e.preventDefault(); setActiveTab("settings"); }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)]">
      <div className="flex items-center gap-0 px-4 py-0 text-[13px] bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)]">
        <div className="flex items-center gap-3 mr-4">
          <strong>Mesh</strong>
          <span className="text-[11px] opacity-70">loading…</span>
        </div>
        <div className="flex items-center gap-0 border-l border-[var(--ct-border)] pl-2">
          {TABS.map((tab) => (
            <button key={tab} onClick={() => setActiveTab(tab)} className={activeTab === tab ? TAB_ACTIVE : TAB_INACTIVE}>{tab}</button>
          ))}
        </div>
      </div>
      <div className="relative flex-1 min-h-0 overflow-auto">
        {activeTab === "map" && <div className="p-4 text-xs text-[var(--ct-muted-fg)]">Map view (Phase 5 Task 5.2)</div>}
        {activeTab === "activity" && <div className="p-4 text-xs text-[var(--ct-muted-fg)]">Activity (Phase 12)</div>}
        {activeTab === "settings" && <div className="p-4 text-xs text-[var(--ct-muted-fg)]">Settings (Phase 9)</div>}
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Wire MeshPanel into the main app router**

Find where MemoryPanel is rendered (search `<MemoryPanel`) and add a parallel `<MeshPanel>` mount tied to a "mesh" route in the toolbar.

- [ ] **Step 5: Verify in dev (visual smoke)**

```bash
./build.sh dev  # restart
# Open app, navigate to Mesh tab — should render header + 3 sub-tabs + placeholder content
```

- [ ] **Step 6: Commit**

```bash
git add src/components/MeshPanel/ src/<router-file>.tsx
git commit -m "feat(mesh): MeshPanel skeleton with sub-tab router (mirrors MemoryPanel pattern)"
```

---

## Phase 6: MeshCanvas + PeerTerminal (terminals-as-terminals)

### Task 6.1: PeerTerminal component

**Files:**
- Create: `src/components/MeshPanel/PeerTerminal.tsx`

- [ ] **Step 1: Implement terminal-window-shaped peer node**

```tsx
// src/components/MeshPanel/PeerTerminal.tsx
import type { Service } from "./types";

interface Props {
  name: string;
  ip: string;
  isThisMachine?: boolean;
  isPaired?: boolean;
  accessLevel?: "full" | "read_only" | "agent_isolated";
  services: Service[];
  onClick?: () => void;
  onAddService?: () => void;
}

const HEALTH_DOT_COLOR: Record<string, string> = {
  healthy: "var(--ct-green)",
  degraded: "var(--ct-yellow)",
  down: "var(--ct-red)",
  unknown: "var(--ct-muted)",
};

export function PeerTerminal({ name, ip, isThisMachine, isPaired, accessLevel, services, onClick, onAddService }: Props) {
  return (
    <div
      onClick={onClick}
      className={`bg-[var(--ct-bg)] border rounded-md cursor-pointer transition-shadow ${
        isThisMachine
          ? "border-2 border-[var(--ct-accent)] shadow-[0_8px_24px_rgba(203,166,247,0.2)]"
          : "border border-[var(--ct-border)] shadow-md"
      }`}
      style={{ width: isThisMachine ? 280 : 230 }}
    >
      <div className="flex items-center gap-1.5 px-2.5 py-1.5 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] rounded-t-md">
        <span className="w-2.5 h-2.5 rounded-full bg-[#f38ba8]" />
        <span className="w-2.5 h-2.5 rounded-full bg-[#f9e2af]" />
        <span className="w-2.5 h-2.5 rounded-full bg-[#a6e3a1]" />
        <span className="ml-1.5 text-[11px] text-[var(--ct-fg)] font-medium">{name}</span>
        <span className="ml-auto text-[9px] text-[var(--ct-muted)] font-mono">{ip}{isThisMachine ? " · this" : ""}</span>
      </div>
      <div className="px-2.5 py-1.5 text-[10px]" style={{ color: "var(--ct-muted-fg)", fontFamily: "ui-monospace, monospace" }}>
        <div className="text-[var(--ct-muted)] mb-1">$ codeterm services list</div>
        <div className="flex gap-1 flex-wrap">
          {services.map(s => (
            <span key={s.id} className="bg-[var(--ct-titlebar-bg)] px-1.5 py-0.5 rounded text-[9px] inline-flex items-center gap-1"
                  style={{ color: HEALTH_DOT_COLOR[s.health.status] }}>
              <span className="w-[5px] h-[5px] rounded-full" style={{ background: HEALTH_DOT_COLOR[s.health.status] }} />
              {s.name}<span className="opacity-50">·{s.kind}</span>
            </span>
          ))}
          {isThisMachine && onAddService && (
            <button onClick={(e) => { e.stopPropagation(); onAddService(); }}
                    className="bg-transparent text-[var(--ct-accent)] px-1.5 py-0.5 border border-dashed border-[var(--ct-accent)] rounded text-[9px] cursor-pointer">
              + add
            </button>
          )}
        </div>
        {isPaired && accessLevel && (
          <div className="mt-1.5 text-[8px] text-[var(--ct-muted)]">paired · access: {accessLevel}</div>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add src/components/MeshPanel/PeerTerminal.tsx
git commit -m "feat(mesh): PeerTerminal — terminal-window-shaped peer node with services as inline badges"
```

### Task 6.2: useMeshData hook (local + paired-peer fetch + merge)

**Files:**
- Create: `src/components/MeshPanel/useMeshData.ts`

- [ ] **Step 1: Implement hook**

```typescript
// src/components/MeshPanel/useMeshData.ts
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { servicesList } from "./meshApi";
import type { Service } from "./types";

interface PeerStatus { name: string; ip: string; online: boolean; access_level: string; }
interface MeshData {
  thisMachine: { name: string; ip: string };
  peers: PeerStatus[];
  services: Service[]; // includes source_peer for remote
  loading: boolean;
  error: string | null;
}

export function useMeshData(refreshKey: number) {
  const [data, setData] = useState<MeshData>({
    thisMachine: { name: "this", ip: "127.0.0.1" },
    peers: [],
    services: [],
    loading: true,
    error: null,
  });

  const fetch = useCallback(async () => {
    setData(d => ({ ...d, loading: true }));
    try {
      const local: Service[] = await servicesList();
      const status: any = await invoke("mesh_get_status").catch(() => ({}));
      const meshPeers: any = await invoke("mesh_list_peers").catch(() => ({ paired: [] }));
      const paired: PeerStatus[] = (meshPeers.paired ?? []).map((p: any) => ({
        name: p.instance_name,
        ip: p.addresses[0]?.ip ?? "",
        online: p.online,
        access_level: p.access_level,
      }));

      const remotePromises = paired.filter(p => p.online).map(async p => {
        try {
          const r = await fetch(`http://${p.ip}:7685/api/services`, { signal: AbortSignal.timeout(3000) });
          const arr: Service[] = await r.json();
          return arr.map(s => ({ ...s, source_peer: p.name }));
        } catch { return []; }
      });
      const remoteResults = await Promise.all(remotePromises);
      const remote = remoteResults.flat();

      setData({
        thisMachine: { name: status.instance_name ?? "this", ip: status.local_ip ?? status.tailscale_ip ?? "" },
        peers: paired,
        services: [...local, ...remote],
        loading: false,
        error: null,
      });
    } catch (e: any) {
      setData(d => ({ ...d, loading: false, error: String(e) }));
    }
  }, []);

  useEffect(() => { fetch(); }, [fetch, refreshKey]);

  return data;
}
```

- [ ] **Step 2: Commit**

```bash
git add src/components/MeshPanel/useMeshData.ts
git commit -m "feat(mesh): useMeshData hook — fetch local + paired-peer services with merge"
```

### Task 6.3: MeshCanvas — orchestrate PeerTerminals + connection lines

**Files:**
- Create: `src/components/MeshPanel/MeshCanvas.tsx`
- Create: `src/components/MeshPanel/ConnectionLine.tsx`

- [ ] **Step 1: ConnectionLine component (SVG line + animated push pulse)**

```tsx
// src/components/MeshPanel/ConnectionLine.tsx
interface Props {
  fromX: number; fromY: number; toX: number; toY: number;
  active: boolean;     // recent push within last 5min
  pulsing: boolean;    // currently in-flight push
}
export function ConnectionLine({ fromX, fromY, toX, toY, active, pulsing }: Props) {
  return (
    <>
      <line x1={fromX} y1={fromY} x2={toX} y2={toY}
        stroke={active ? "var(--ct-accent)" : "var(--ct-border)"}
        strokeWidth={active ? 1.5 : 0.5} strokeOpacity={active ? 0.8 : 0.4}
        strokeDasharray={active ? "" : "4 3"} />
      {pulsing && (
        <circle r="3" fill="var(--ct-accent)">
          <animateMotion dur="2s" repeatCount="indefinite" path={`M${fromX},${fromY} L${toX},${toY}`} />
        </circle>
      )}
    </>
  );
}
```

- [ ] **Step 2: MeshCanvas with radial layout**

```tsx
// src/components/MeshPanel/MeshCanvas.tsx
import { useMemo } from "react";
import { PeerTerminal } from "./PeerTerminal";
import { ConnectionLine } from "./ConnectionLine";
import type { Service } from "./types";

interface PeerLayout { name: string; ip: string; x: number; y: number; isThisMachine?: boolean; isPaired?: boolean; }

interface Props {
  thisMachine: { name: string; ip: string };
  peers: { name: string; ip: string; online: boolean; access_level: string }[];
  services: Service[];
  recentPushes: { source_id: string; target: string; at: number }[];
  onPeerClick: (peerName: string) => void;
  onAddService: () => void;
  width: number;
  height: number;
}

export function MeshCanvas({ thisMachine, peers, services, recentPushes, onPeerClick, onAddService, width, height }: Props) {
  const layout = useMemo<PeerLayout[]>(() => {
    const cx = width / 2, cy = height / 2;
    const radius = Math.min(width, height) / 2 - 130;
    const items: PeerLayout[] = [{ name: thisMachine.name, ip: thisMachine.ip, x: cx, y: cy, isThisMachine: true }];
    peers.forEach((p, i) => {
      const angle = (i / Math.max(peers.length, 1)) * Math.PI * 2 - Math.PI / 2;
      items.push({ name: p.name, ip: p.ip, x: cx + Math.cos(angle) * radius, y: cy + Math.sin(angle) * radius, isPaired: true });
    });
    return items;
  }, [thisMachine, peers, width, height]);

  const local = layout[0];
  const now = Date.now();

  return (
    <div className="relative" style={{ minHeight: height, background: "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)" }}>
      <svg className="absolute inset-0 pointer-events-none">
        {layout.slice(1).map((p, i) => {
          const recent = recentPushes.some(rp => rp.source_id.includes(p.name) || rp.target.includes(p.name) ? (now - rp.at) < 300_000 : false);
          const pulsing = recentPushes.some(rp => (now - rp.at) < 3000);
          return <ConnectionLine key={i} fromX={local.x} fromY={local.y} toX={p.x} toY={p.y} active={recent} pulsing={pulsing} />;
        })}
      </svg>
      {layout.map((p, i) => (
        <div key={i} style={{ position: "absolute", left: p.x, top: p.y, transform: "translate(-50%,-50%)" }}>
          <PeerTerminal
            name={p.name} ip={p.ip}
            isThisMachine={p.isThisMachine}
            isPaired={p.isPaired}
            services={services.filter(s => p.isThisMachine ? !s.source_peer : s.source_peer === p.name)}
            onClick={() => onPeerClick(p.name)}
            onAddService={p.isThisMachine ? onAddService : undefined}
          />
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 3: Commit**

```bash
git add src/components/MeshPanel/MeshCanvas.tsx src/components/MeshPanel/ConnectionLine.tsx
git commit -m "feat(mesh): MeshCanvas with radial peer layout + ConnectionLine animated push pulse"
```

---

## Phase 7: ServicesTable + ServiceRow

### Task 7.1: Flat services table

**Files:**
- Create: `src/components/MeshPanel/ServicesTable.tsx`
- Create: `src/components/MeshPanel/ServiceRow.tsx`

- [ ] **Step 1: ServiceRow component**

```tsx
// src/components/MeshPanel/ServiceRow.tsx
import type { Service } from "./types";

const STATUS_COLOR: Record<string, string> = {
  healthy: "var(--ct-green)", degraded: "var(--ct-yellow)",
  down: "var(--ct-red)", unknown: "var(--ct-muted)",
};

interface Props { service: Service; host: string; onClick: () => void; }

export function ServiceRow({ service, host, onClick }: Props) {
  return (
    <div onClick={onClick}
      className="grid items-center px-4 py-1.5 border-b border-[var(--ct-border)] cursor-pointer hover:bg-[var(--ct-titlebar-bg)]"
      style={{ gridTemplateColumns: "14px 2fr 1fr 1fr 80px 70px 24px" }}>
      <span style={{ color: STATUS_COLOR[service.health.status] }}>●</span>
      <span className="text-[var(--ct-fg)]">{service.name}</span>
      <span className="text-[var(--ct-muted)] text-[11px]">{host}</span>
      <span className="text-[var(--ct-muted)] text-[11px]">{service.kind} · {service.health_mode}</span>
      <span className="text-[10px]" style={{ color: STATUS_COLOR[service.health.status] }}>{service.health.status}</span>
      <span className="text-[var(--ct-muted)] text-[10px]">
        {service.health.uptime_secs ? `${Math.floor(service.health.uptime_secs / 3600)}h` : "-"}
      </span>
      <span className="text-[var(--ct-muted)]">›</span>
    </div>
  );
}
```

- [ ] **Step 2: ServicesTable component**

```tsx
// src/components/MeshPanel/ServicesTable.tsx
import { useState } from "react";
import { ServiceRow } from "./ServiceRow";
import type { Service } from "./types";

interface Props {
  services: Service[];
  thisMachineName: string;
  onServiceClick: (service: Service) => void;
}

export function ServicesTable({ services, thisMachineName, onServiceClick }: Props) {
  const [hostFilter, setHostFilter] = useState<string>("all");
  const [kindFilter, setKindFilter] = useState<string>("all");
  const [statusFilter, setStatusFilter] = useState<string>("all");

  const hosts = Array.from(new Set([thisMachineName, ...services.map(s => s.source_peer ?? thisMachineName)]));
  const filtered = services.filter(s => {
    const host = s.source_peer ?? thisMachineName;
    if (hostFilter !== "all" && host !== hostFilter) return false;
    if (kindFilter !== "all" && s.kind !== kindFilter) return false;
    if (statusFilter !== "all" && s.health.status !== statusFilter) return false;
    return true;
  });

  return (
    <div className="bg-[var(--ct-bg)] border-t border-[var(--ct-border)]">
      <div className="flex items-center px-4 py-2 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[10px] uppercase text-[var(--ct-muted)]">
        <span className="flex-1">All services ({filtered.length} of {services.length})</span>
        <select value={hostFilter} onChange={e => setHostFilter(e.target.value)} className="text-[10px] mr-3 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5">
          <option value="all">all hosts</option>
          {hosts.map(h => <option key={h} value={h}>{h}</option>)}
        </select>
        <select value={kindFilter} onChange={e => setKindFilter(e.target.value)} className="text-[10px] mr-3 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5">
          <option value="all">all kinds</option>
          <option value="cron">cron</option>
          <option value="daemon">daemon</option>
        </select>
        <select value={statusFilter} onChange={e => setStatusFilter(e.target.value)} className="text-[10px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5">
          <option value="all">all statuses</option>
          <option value="healthy">healthy</option>
          <option value="degraded">degraded</option>
          <option value="down">down</option>
        </select>
      </div>
      <div className="grid items-center px-4 py-1.5 border-b border-[var(--ct-border)] text-[9px] uppercase text-[var(--ct-muted)]"
        style={{ gridTemplateColumns: "14px 2fr 1fr 1fr 80px 70px 24px" }}>
        <span /><span>Name</span><span>Host</span><span>Type · Mode</span><span>Status</span><span>Uptime</span><span />
      </div>
      {filtered.map(s => (
        <ServiceRow key={s.id} service={s} host={s.source_peer ?? thisMachineName} onClick={() => onServiceClick(s)} />
      ))}
      {filtered.length === 0 && (
        <div className="px-4 py-3 text-center text-[var(--ct-muted)] text-xs">No services match filter.</div>
      )}
    </div>
  );
}
```

- [ ] **Step 3: Commit**

```bash
git add src/components/MeshPanel/ServicesTable.tsx src/components/MeshPanel/ServiceRow.tsx
git commit -m "feat(mesh): ServicesTable with host/kind/status filter chips + ServiceRow"
```

### Task 7.2: Wire MeshCanvas + ServicesTable into MeshPanel map view

**Files:**
- Modify: `src/components/MeshPanel/MeshPanel.tsx`

- [ ] **Step 1: Replace placeholder map content with real components**

Replace the `{activeTab === "map" && ...}` block:
```tsx
{activeTab === "map" && (
  <div className="flex flex-col h-full">
    <div className="flex-shrink-0" style={{ height: 400 }}>
      <MeshCanvas
        thisMachine={data.thisMachine}
        peers={data.peers}
        services={data.services}
        recentPushes={[]}
        onPeerClick={(name) => { setSelectedDrawer({ kind: "peer", id: name }); }}
        onAddService={() => setShowAddServiceWizard(true)}
        width={800}
        height={400}
      />
    </div>
    <div className="flex-1 overflow-auto">
      <ServicesTable
        services={data.services}
        thisMachineName={data.thisMachine.name}
        onServiceClick={(s) => setSelectedDrawer({ kind: "service", id: s.id })}
      />
    </div>
  </div>
)}
```

Add the relevant state hooks (`useMeshData(refreshKey)`, `selectedDrawer`, `showAddServiceWizard`).

- [ ] **Step 2: Smoke test in dev**

```bash
./build.sh dev
# Expected: Mesh tab shows canvas with this-machine + paired peers, services table below
```

- [ ] **Step 3: Commit**

```bash
git add src/components/MeshPanel/MeshPanel.tsx
git commit -m "feat(mesh): wire MeshCanvas + ServicesTable into map sub-tab"
```

---

## Phase 8: ServiceDrawer (peer + service contexts)

### Task 8.1: Drawer shell + peer context

**Files:**
- Create: `src/components/MeshPanel/ServiceDrawer.tsx`

- [ ] **Step 1: Implement drawer**

```tsx
// src/components/MeshPanel/ServiceDrawer.tsx
import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import type { Service } from "./types";

interface PeerContext { kind: "peer"; id: string; }
interface ServiceContext { kind: "service"; id: string; }
type DrawerState = PeerContext | ServiceContext | null;

interface Props {
  state: DrawerState;
  services: Service[];
  peers: any[];
  onClose: () => void;
  onActionDone: () => void;
}

export function ServiceDrawer({ state, services, peers, onClose, onActionDone }: Props) {
  if (!state) return null;
  if (state.kind === "peer") return <PeerDrawer peer={peers.find(p => p.instance_name === state.id)} services={services.filter(s => s.source_peer === state.id)} onClose={onClose} onActionDone={onActionDone} />;
  return <SvcDrawer service={services.find(s => s.id === state.id)!} onClose={onClose} onActionDone={onActionDone} />;
}

function PeerDrawer({ peer, services, onClose, onActionDone }: { peer: any; services: Service[]; onClose: () => void; onActionDone: () => void }) {
  if (!peer) return null;
  const handleDisconnect = async () => {
    await invoke("mesh_disconnect", { instanceId: peer.instance_id });
    onActionDone(); onClose();
  };
  return (
    <div className="fixed top-0 right-0 bottom-0 w-[360px] bg-[var(--ct-titlebar-bg)] border-l border-[var(--ct-border)] p-4 flex flex-col gap-3 z-50">
      <div className="flex items-center justify-between">
        <strong className="text-[var(--ct-fg)]">{peer.instance_name}</strong>
        <button onClick={onClose} className="text-[var(--ct-muted)] cursor-pointer">×</button>
      </div>
      <div className="flex gap-1.5">
        <span className="bg-[var(--ct-green)] text-[var(--ct-bg)] px-1.5 py-0.5 rounded text-[9px] font-medium">paired</span>
        <span className="bg-[var(--ct-bg)] text-[var(--ct-muted)] px-1.5 py-0.5 rounded text-[9px]">{peer.access_level}</span>
      </div>
      <div className="text-[10px]"><div className="text-[var(--ct-muted)] mb-1">IP</div><div className="font-mono">{peer.addresses[0]?.ip}:7685</div></div>
      <div className="text-[10px]">
        <div className="text-[var(--ct-muted)] mb-1">Services on this peer ({services.length})</div>
        <div className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded p-2 flex flex-col gap-1">
          {services.map(s => <div key={s.id}>● {s.name} <span className="text-[var(--ct-muted)]">· {s.kind}</span></div>)}
          {services.length === 0 && <div className="text-[var(--ct-muted)] italic">none registered</div>}
        </div>
      </div>
      <div className="mt-auto flex gap-1.5">
        <button onClick={onActionDone} className="flex-1 py-1.5 bg-[var(--ct-bg)] text-[var(--ct-fg)] border-none rounded text-[10px] cursor-pointer">Refresh</button>
        <button onClick={handleDisconnect} className="flex-1 py-1.5 bg-[var(--ct-red)] text-[var(--ct-bg)] border-none rounded text-[10px] cursor-pointer font-medium">Disconnect</button>
      </div>
    </div>
  );
}

function SvcDrawer({ service, onClose, onActionDone }: { service: Service; onClose: () => void; onActionDone: () => void }) {
  const [showToken, setShowToken] = useState<string | null>(null);
  const handleProbe = async () => { await invoke("services_probe", { id: service.id }); onActionDone(); };
  const handleDelete = async () => { if (!confirm("Delete service?")) return; await invoke("services_delete", { id: service.id }); onActionDone(); onClose(); };
  const handleRotate = async () => { const tk = await invoke<string>("services_regenerate_token", { id: service.id }); setShowToken(tk); };
  return (
    <div className="fixed top-0 right-0 bottom-0 w-[400px] bg-[var(--ct-titlebar-bg)] border-l border-[var(--ct-border)] p-4 flex flex-col gap-3 z-50 overflow-auto">
      <div className="flex items-center justify-between">
        <strong className="text-[var(--ct-fg)]">{service.name}</strong>
        <button onClick={onClose} className="text-[var(--ct-muted)] cursor-pointer">×</button>
      </div>
      <div className="text-[10px]"><div className="text-[var(--ct-muted)]">Kind</div><div>{service.kind}</div></div>
      <div className="text-[10px]"><div className="text-[var(--ct-muted)]">Health</div><div>{service.health.status} · v{service.health.version ?? "?"} · uptime {service.health.uptime_secs ?? 0}s</div></div>
      {service.url && <div className="text-[10px]"><div className="text-[var(--ct-muted)]">URL</div><div className="font-mono">{service.url}</div></div>}
      {showToken && (
        <div className="bg-[var(--ct-bg)] border border-[var(--ct-yellow)] rounded p-2 text-[10px]">
          <div className="text-[var(--ct-yellow)] font-bold mb-1">New token (save NOW):</div>
          <div className="font-mono break-all select-all">{showToken}</div>
        </div>
      )}
      <div className="mt-auto flex flex-col gap-1.5">
        <button onClick={handleProbe} className="py-1.5 bg-[var(--ct-bg)] text-[var(--ct-fg)] border-none rounded text-[10px] cursor-pointer">Probe now</button>
        <button onClick={handleRotate} className="py-1.5 bg-[var(--ct-bg)] text-[var(--ct-fg)] border-none rounded text-[10px] cursor-pointer">Rotate token</button>
        <button onClick={handleDelete} className="py-1.5 bg-[var(--ct-red)] text-[var(--ct-bg)] border-none rounded text-[10px] cursor-pointer font-medium">Delete service</button>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Wire into MeshPanel**

In `MeshPanel.tsx` add:
```tsx
const [drawerState, setDrawerState] = useState<DrawerState>(null);
// ... in JSX after the map content:
<ServiceDrawer state={drawerState} services={data.services} peers={data.peers} onClose={() => setDrawerState(null)} onActionDone={() => setRefreshKey(k => k + 1)} />
```

- [ ] **Step 3: Commit**

```bash
git add src/components/MeshPanel/ServiceDrawer.tsx src/components/MeshPanel/MeshPanel.tsx
git commit -m "feat(mesh): ServiceDrawer with peer + service contexts (probe/rotate/delete)"
```

---

## Phase 9: AddServiceWizard (manual path; Smart-Add deferred to Phase 13)

### Task 9.1: Manual add wizard modal

**Files:**
- Create: `src/components/MeshPanel/AddServiceWizard.tsx`

- [ ] **Step 1: Implement wizard**

```tsx
// src/components/MeshPanel/AddServiceWizard.tsx
import { useState } from "react";
import { servicesCreate } from "./meshApi";
import type { HealthMode } from "./types";

interface Props { onClose: () => void; onCreated: () => void; }

export function AddServiceWizard({ onClose, onCreated }: Props) {
  const [name, setName] = useState("");
  const [healthMode, setHealthMode] = useState<HealthMode>("http");
  const [url, setUrl] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [processLabel, setProcessLabel] = useState("");
  const [scriptPath, setScriptPath] = useState("");
  const [pushEnabled, setPushEnabled] = useState(false);
  const [token, setToken] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async () => {
    setSubmitting(true);
    try {
      const result = await servicesCreate({
        name,
        kind: "daemon",
        health_mode: healthMode,
        url: healthMode === "http" ? url : undefined,
        host: healthMode === "tcp" ? host : undefined,
        port: healthMode === "tcp" ? parseInt(port, 10) : undefined,
        process_label: healthMode === "process" ? processLabel : undefined,
        script_path: healthMode === "script" ? scriptPath : undefined,
        push_enabled: pushEnabled,
      });
      if (pushEnabled && result.token) {
        setToken(result.token);
      } else {
        onCreated(); onClose();
      }
    } catch (e: any) {
      alert(`Failed: ${e}`);
    }
    setSubmitting(false);
  };

  if (token) {
    return (
      <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50" onClick={onClose}>
        <div onClick={e => e.stopPropagation()} className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-md p-6 w-[480px]">
          <h2 className="text-[var(--ct-fg)] mb-3">Service registered ✓</h2>
          <p className="text-xs text-[var(--ct-yellow)] mb-2">Push token (save this NOW — will not be shown again):</p>
          <div className="bg-[var(--ct-titlebar-bg)] p-3 rounded font-mono text-xs select-all break-all border border-[var(--ct-yellow)]">{token}</div>
          <button onClick={() => { onCreated(); onClose(); }} className="mt-4 px-4 py-2 bg-[var(--ct-accent)] text-[var(--ct-bg)] rounded text-xs">Done</button>
        </div>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50" onClick={onClose}>
      <div onClick={e => e.stopPropagation()} className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-md p-5 w-[480px] flex flex-col gap-3">
        <h2 className="text-[var(--ct-fg)]">Add Service</h2>
        <Field label="Name"><input value={name} onChange={e => setName(e.target.value)} className="bg-[var(--ct-titlebar-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-xs w-full text-[var(--ct-fg)]" /></Field>
        <Field label="Health mode">
          <select value={healthMode} onChange={e => setHealthMode(e.target.value as HealthMode)} className="bg-[var(--ct-titlebar-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-xs w-full text-[var(--ct-fg)]">
            <option value="http">HTTP</option><option value="tcp">TCP</option>
            <option value="process">Process (launchd / systemd)</option><option value="script">Script</option>
          </select>
        </Field>
        {healthMode === "http" && <Field label="URL"><input placeholder="http://127.0.0.1:7891/healthz" value={url} onChange={e => setUrl(e.target.value)} className="bg-[var(--ct-titlebar-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-xs w-full font-mono" /></Field>}
        {healthMode === "tcp" && <>
          <Field label="Host"><input value={host} onChange={e => setHost(e.target.value)} className="..." /></Field>
          <Field label="Port"><input value={port} onChange={e => setPort(e.target.value)} className="..." /></Field>
        </>}
        {healthMode === "process" && <Field label="Process label (launchd/systemd)"><input value={processLabel} onChange={e => setProcessLabel(e.target.value)} className="..." /></Field>}
        {healthMode === "script" && <Field label="Script path"><input value={scriptPath} onChange={e => setScriptPath(e.target.value)} className="..." /></Field>}
        <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={pushEnabled} onChange={e => setPushEnabled(e.target.checked)} /> Enable push API</label>
        <div className="flex gap-2 mt-2">
          <button onClick={handleSubmit} disabled={submitting || !name} className="px-4 py-2 bg-[var(--ct-accent)] text-[var(--ct-bg)] rounded text-xs disabled:opacity-50">Register</button>
          <button onClick={onClose} className="px-4 py-2 bg-transparent text-[var(--ct-muted)] rounded text-xs">Cancel</button>
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <div className="flex flex-col gap-1"><label className="text-[10px] text-[var(--ct-muted)] uppercase">{label}</label>{children}</div>;
}
```

- [ ] **Step 2: Wire into MeshPanel**

```tsx
const [showAddWizard, setShowAddWizard] = useState(false);
// in JSX:
{showAddWizard && <AddServiceWizard onClose={() => setShowAddWizard(false)} onCreated={() => setRefreshKey(k => k + 1)} />}
```

Connect to "+ Service" header button: `<button onClick={() => setShowAddWizard(true)}>+ Service</button>`.

- [ ] **Step 3: Commit**

```bash
git add src/components/MeshPanel/AddServiceWizard.tsx src/components/MeshPanel/MeshPanel.tsx
git commit -m "feat(mesh): AddServiceWizard manual path (4 health modes + token reveal on push-enabled)"
```

---

## Phase 10: MeshSettingsTab (port from MeshSection.tsx)

### Task 10.1: Direct port preserving all 460-line functionality

**Files:**
- Create: `src/components/MeshPanel/MeshSettingsTab.tsx`
- Delete: `src/components/Settings/MeshSection.tsx`

- [ ] **Step 1: Copy MeshSection.tsx → MeshSettingsTab.tsx, rename component, adjust imports**

```bash
cp src/components/Settings/MeshSection.tsx src/components/MeshPanel/MeshSettingsTab.tsx
# Then edit: rename function, fix import paths (../shared instead of ./shared, etc.)
```

In MeshSettingsTab.tsx, change the export name and adjust the relative imports for `Toggle`, `CopyRow`, and `lib/types`. Keep ALL functionality intact (every invoke call, useState, useEffect).

- [ ] **Step 2: Wire into MeshPanel**

Replace the placeholder `{activeTab === "settings"}` with:
```tsx
{activeTab === "settings" && <MeshSettingsTab config={config} onUpdate={onConfigUpdate} apiEnabled={apiEnabled} />}
```

(Pass through `config`, `onConfigUpdate`, `apiEnabled` from parent.)

- [ ] **Step 3: Delete the old Settings/MeshSection.tsx + remove its import from SettingsPanel.tsx**

- [ ] **Step 4: Verify peer-management still works in dev**

```bash
./build.sh dev
# Open Mesh tab → settings sub-tab → verify all functionality:
# - Device name input edits, broadcast Start/Stop, Refresh, peer Approve/Reject/Disconnect,
#   manual add, exit mesh, copy buttons.
```

- [ ] **Step 5: Commit**

```bash
git add src/components/MeshPanel/MeshSettingsTab.tsx src/components/Settings/SettingsPanel.tsx
git rm src/components/Settings/MeshSection.tsx
git commit -m "feat(mesh): port MeshSection.tsx to MeshSettingsTab.tsx inside Mesh tab"
```

---

## Phase 11: Settings reorganization (Memory + Mesh moved out, slim Settings)

### Task 11.1: Add Tabs section + remove Memory/Mesh from Settings

**Files:**
- Modify: `src/components/Settings/SettingsPanel.tsx`

- [ ] **Step 1: Add Tabs toggle section**

Replace the section list in SettingsPanel.tsx:
```tsx
<section>
  <h3>Tabs</h3>
  <Toggle label="Enable Memory tab" checked={config.ui?.memory_tab_enabled ?? true} onChange={v => onUpdate("ui", { memory_tab_enabled: v })} />
  <button onClick={() => navigateToTab("memory")} className="text-[var(--ct-accent)] text-[10px] hover:underline ml-6">Go to Memory →</button>
  <Toggle label="Enable Mesh tab" checked={config.ui?.mesh_tab_enabled ?? true} onChange={v => {
    onUpdate("ui", { mesh_tab_enabled: v });
    if (v && !config.mesh?.enabled) onUpdate("mesh", { enabled: true });
  }} />
  <button onClick={() => navigateToTab("mesh")} className="text-[var(--ct-accent)] text-[10px] hover:underline ml-6">Go to Mesh →</button>
</section>
```

Remove the old `<MemorySection>` and (already-deleted) MeshSection imports.

- [ ] **Step 2: Add `ui` config schema**

In Rust `src-tauri/src/config/schema.rs`:
```rust
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub memory_tab_enabled: bool,
    #[serde(default = "default_true")]
    pub mesh_tab_enabled: bool,
}
fn default_true() -> bool { true }

// And add ui: UiConfig field to AppConfig.
```

- [ ] **Step 3: Toolbar gates Memory/Mesh buttons on these toggles**

In the toolbar component (search for `MemoryPanel` button), wrap with `{config.ui?.memory_tab_enabled && <button>...Memory</button>}`.

- [ ] **Step 4: Smoke test**

```bash
./build.sh dev
# Open Settings → toggle Mesh OFF → verify Mesh button hidden in toolbar.
# Toggle ON → button reappears, click "Go to Mesh →" navigates correctly.
```

- [ ] **Step 5: Commit**

```bash
git add src/components/Settings/SettingsPanel.tsx src-tauri/src/config/schema.rs <toolbar-file>
git commit -m "feat(settings): slim Settings — Tabs section toggles Memory/Mesh + Go-to deep-links"
```

### Task 11.2: Memory tab settings sub-tab

**Files:**
- Modify: `src/components/MemoryPanel/MemoryPanel.tsx` (add `settings` sub-tab)
- Create: `src/components/MemoryPanel/MemorySettingsTab.tsx`
- Modify: `src/components/Settings/SettingsPanel.tsx` (delete MemorySection import + render)

- [ ] **Step 1: Move existing MemorySection.tsx → MemoryPanel/MemorySettingsTab.tsx**

```bash
git mv src/components/Settings/MemorySection.tsx src/components/MemoryPanel/MemorySettingsTab.tsx
# Adjust imports + rename component.
```

- [ ] **Step 2: Add `settings` to MemoryTab union + render**

In `MemoryPanel.tsx`:
```tsx
export type MemoryTab = "dreams" | "graph" | "list" | "activity" | "settings";
const TABS: MemoryTab[] = ["dreams", "graph", "list", "activity", "settings"];
// ...
{activeTab === "settings" && <MemorySettingsTab config={config} onUpdate={onConfigUpdate} />}
```

- [ ] **Step 3: Add Librarian toggle to General settings, default-on**

In `src/components/Settings/SettingsPanel.tsx` (or General section component):
```tsx
<Toggle label="Enable Librarian (background skill vectorizer)" checked={config.general?.librarian_enabled ?? true} onChange={v => onUpdate("general", { librarian_enabled: v })} />
```

Update Rust schema with `librarian_enabled: bool` (default true).

- [ ] **Step 4: Smoke test**

```bash
./build.sh dev
# Open Memory tab → settings sub-tab → verify all memory toggles present.
# Open Settings → verify Memory section gone, Librarian toggle in General.
```

- [ ] **Step 5: Commit**

```bash
git add src/components/MemoryPanel/ src/components/Settings/ src-tauri/src/config/schema.rs
git commit -m "feat(settings): Memory absorbs its settings into 'settings' sub-tab; Librarian default-on in General"
```

---

## Phase 12: Activity sub-tab + useActivityLog

### Task 12.1: useActivityLog hook + ActivityTab component

**Files:**
- Create: `src/components/MeshPanel/useActivityLog.ts`
- Create: `src/components/MeshPanel/ActivityTab.tsx`

- [ ] **Step 1: Hook**

```typescript
// src/components/MeshPanel/useActivityLog.ts
import { useEffect, useState } from "react";
import { activityList } from "./meshApi";
import type { ActivityEvent } from "./types";

export function useActivityLog(active: boolean) {
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let lastSeen: number | undefined = undefined;
    const tick = async () => {
      const latest = await activityList({ since: lastSeen, limit: 200 }).catch(() => []);
      if (cancelled) return;
      if (latest.length > 0) {
        lastSeen = latest[0].occurred_at;
        setEvents(prev => [...latest.reverse(), ...prev].slice(0, 500));
      }
    };
    tick();
    const handle = setInterval(tick, 5000);
    return () => { cancelled = true; clearInterval(handle); };
  }, [active]);
  return events;
}
```

- [ ] **Step 2: ActivityTab**

```tsx
// src/components/MeshPanel/ActivityTab.tsx
import { useActivityLog } from "./useActivityLog";
import type { ActivityEvent } from "./types";

const SOURCE_ICON: Record<string, string> = { push: "⤳", health: "●", peer: "↔", cron: "⏱" };
const LEVEL_COLOR: Record<string, string> = { info: "var(--ct-fg)", warn: "var(--ct-yellow)", error: "var(--ct-red)" };

interface Props { active: boolean; }
export function ActivityTab({ active }: Props) {
  const events = useActivityLog(active);
  if (events.length === 0) return <div className="p-4 text-[var(--ct-muted)] text-xs">No activity yet.</div>;
  return (
    <div className="overflow-auto">
      {events.map(ev => (
        <div key={ev.id} className="grid items-center px-4 py-1.5 border-b border-[var(--ct-border)]"
          style={{ gridTemplateColumns: "70px 100px 18px 1fr", color: LEVEL_COLOR[ev.level] }}>
          <span className="text-[10px] text-[var(--ct-muted)] font-mono">{new Date(ev.occurred_at).toLocaleTimeString()}</span>
          <span className="text-[10px] text-[var(--ct-muted)]">{ev.source_peer ?? "this"}</span>
          <span>{SOURCE_ICON[ev.source]}</span>
          <span className="text-xs">{ev.message}</span>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 3: Wire into MeshPanel**

```tsx
{activeTab === "activity" && <ActivityTab active={activeTab === "activity"} />}
```

- [ ] **Step 4: Add `activity_list` Tauri command in Rust**

```rust
// src-tauri/src/commands/services.rs (add to existing)
#[tauri::command]
pub fn activity_list(
    svc: tauri::State<'_, crate::service::ServiceState>,
    source: Option<String>,
    since: Option<i64>,
    limit: Option<usize>,
) -> Result<Vec<crate::services::activity::ActivityEvent>, String> {
    let writer = crate::services::activity::ActivityWriter::new(svc.services_store.conn());
    let src = source.and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok());
    writer.list(src, since, limit).map_err(|e| e.to_string())
}
```

Register in `lib.rs` invoke_handler.

- [ ] **Step 5: Commit**

```bash
git add src/components/MeshPanel/useActivityLog.ts src/components/MeshPanel/ActivityTab.tsx src-tauri/src/commands/services.rs src-tauri/src/lib.rs
git commit -m "feat(mesh): activity sub-tab + useActivityLog 5s polling"
```

---

## Phase 13: Smart-Add (AI-agent-assisted field mapping)

### Task 13.1: Backend `services_smart_add_inspect` command + agent worker

**Files:**
- Create: `src-tauri/src/services/smart_add.rs`
- Modify: `src-tauri/src/commands/services.rs`
- Modify: `src/components/MeshPanel/AddServiceWizard.tsx`

- [ ] **Step 1: Backend smart-add inspector**

```rust
// src-tauri/src/services/smart_add.rs
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug)]
pub struct SmartAddResult {
    pub suggested_name: Option<String>,
    pub recommended_health_mode: String,
    pub recommended_health_path: Option<String>,
    pub field_mapping: serde_json::Value,
    pub raw_response: serde_json::Value,
}

pub async fn inspect(url: &str) -> Result<SmartAddResult, String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let body = response.text().await.unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::json!({}));

    // Heuristics — agent call deferred to higher tier; for v1 simple key-presence detection.
    let mut field_mapping = serde_json::Map::new();
    if parsed.get("status").is_some() { field_mapping.insert("ui_status".into(), serde_json::json!("$.status")); }
    if parsed.get("version").is_some() { field_mapping.insert("ui_version".into(), serde_json::json!("$.version")); }
    if parsed.get("uptime").is_some() { field_mapping.insert("ui_uptime".into(), serde_json::json!("$.uptime")); }

    Ok(SmartAddResult {
        suggested_name: None,
        recommended_health_mode: "http".into(),
        recommended_health_path: Some("/healthz".into()),
        field_mapping: serde_json::Value::Object(field_mapping),
        raw_response: parsed,
    })
}
```

(Note: the spec mentions Haiku-based field-mapping; for v1 use heuristics. Upgrade to LLM-based agent worker as a future enhancement.)

- [ ] **Step 2: Add Tauri command + wire into wizard "Inspect URL" button**

Add `services_smart_add_inspect` Tauri command. In AddServiceWizard add an "Inspect URL" button that calls it and pre-fills form fields from the result.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/services/smart_add.rs src-tauri/src/services/mod.rs src-tauri/src/commands/services.rs src/components/MeshPanel/AddServiceWizard.tsx
git commit -m "feat(mesh): Smart-Add inspector — heuristic field-mapping suggestions from /healthz"
```

---

## Phase 14: Connection-line push activity wiring

### Task 14.1: Listen for service-health-changed + push events

**Files:**
- Modify: `src/components/MeshPanel/MeshPanel.tsx`

- [ ] **Step 1: Subscribe to Tauri events for live updates**

```tsx
useEffect(() => {
  let unlistenHealth: (() => void) | null = null;
  let unlistenPush: (() => void) | null = null;
  import("@tauri-apps/api/event").then(({ listen }) => {
    listen<{ service_id: string; status: string }>("service-health-changed", () => {
      setRefreshKey(k => k + 1);
    }).then(fn => unlistenHealth = fn);
    listen<{ source_id: string; target: string }>("service-pushed", (ev) => {
      setRecentPushes(prev => [...prev, { ...ev.payload, at: Date.now() }].slice(-20));
    }).then(fn => unlistenPush = fn);
  });
  return () => { unlistenHealth?.(); unlistenPush?.(); };
}, []);
```

- [ ] **Step 2: Backend emits service-pushed in push handler (pane_routes.rs)**

After successful pane write with svc_token_match:
```rust
let _ = tauri::Emitter::emit(&state.app_handle, "service-pushed",
    serde_json::json!({"source_id": svc_id, "target": pane_id}));
```

- [ ] **Step 3: Commit**

```bash
git add src/components/MeshPanel/MeshPanel.tsx src-tauri/src/api/pane_routes.rs
git commit -m "feat(mesh): wire service-health-changed + service-pushed Tauri events for live canvas updates"
```

---

## Phase 15: Cron panel deletion

### Task 15.1: Delete CronPanel + toolbar entry

**Files:**
- Delete: `src/components/CronPanel/`
- Modify: toolbar component (remove cron button)

- [ ] **Step 1: Verify cron functionality fully accessible via Mesh drawer**

Manually test: open Mesh tab, click any cron-kind row → drawer shows schedule, history, log, with edit/cancel/trigger buttons working through `/api/crons/:id` endpoints.

- [ ] **Step 2: Delete CronPanel directory**

```bash
git rm -r src/components/CronPanel/
```

- [ ] **Step 3: Remove toolbar button + import**

In toolbar component, remove `<button>Crons</button>` and the `import { CronPanel }` line.

- [ ] **Step 4: Smoke test**

```bash
./build.sh dev
# Verify Crons button gone from toolbar, cron management still works via Mesh tab drawer.
```

- [ ] **Step 5: Commit**

```bash
git add -A src/components/ <toolbar-file>
git commit -m "refactor(cron): delete CronPanel — cron management now lives in Mesh tab drawer"
```

---

## Phase 16: Documentation

### Task 16.1: Update docs

**Files:**
- Modify: `docs/API.md` (add /api/services + /api/activity endpoints)
- Modify: `docs/MESH.md` (add Mesh tab section + service registry concept)
- Modify: `docs/CONFIG.md` (add ui.memory_tab_enabled / ui.mesh_tab_enabled / general.librarian_enabled)
- Modify: `skills/codeterm/SKILL.md` (add `codeterm service` command reference)
- Modify: `src-tauri/prompts/general_agent.md` (add Services section pointing at `--help`)
- Modify: `CLAUDE.md` (add "Services panel" to Doc list)

- [ ] **Step 1: API.md — add Services endpoints table**

Add a new table section `## Services API` mirroring the Cron API style, listing all `/api/services/*` and `/api/activity` endpoints.

- [ ] **Step 2: MESH.md — add concept section**

Add a "## Mesh tab and service registry" section explaining: Service entity (kind=cron|daemon), pull-on-demand discovery, per-service push tokens, activity log.

- [ ] **Step 3: General agent prompt addition**

```markdown
### Services / daemons in mesh

Each codeterm instance has a Mesh tab listing local services (cron + daemon)
and services on paired peers. Daemons register via `codeterm service add`
(see `--help` for current flags). For push-back to a pane, the daemon uses
the per-service token as `Authorization: Bearer <tk>` on
`POST /api/panes/<pane_id>/send`.

When the user asks "what services are running?" — `codeterm service list`.
For peer services across the mesh, the Mesh tab UI is the canonical surface;
CLI `codeterm service list --mesh PEER` queries a single peer.
```

- [ ] **Step 4: Bump revision tags on changed docs**

For each modified `docs/*.md`, increment the `<!-- revision: N -->` tag.

- [ ] **Step 5: Commit**

```bash
git add docs/ skills/codeterm/SKILL.md src-tauri/prompts/general_agent.md CLAUDE.md
git commit -m "docs(mesh): API + MESH + CONFIG + skill + GA prompt updates for Services/Mesh tab"
```

### Task 16.2: Final integration test pass

- [ ] **Step 1: Run full Rust test suite**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast 2>&1 | tail -10
```
Expected: all green.

- [ ] **Step 2: Run frontend tests**

```bash
npm test 2>&1 | tail -10
```
Expected: all green.

- [ ] **Step 3: Run E2E smoke**

```bash
./build.sh e2e 2>&1 | tail -20
```
Expected: critical paths pass (open Mesh tab, add service, push from daemon, see in pane).

- [ ] **Step 4: Final commit if any leftover artifacts**

```bash
git status
git diff
# If clean, no commit needed. Otherwise commit with a "test: final integration pass" message.
```

---

## Self-review summary

This plan covers all 16 phases from spec §8.4. Each phase is independently revertable:

- **Phases 1-4** are pure backend additions. No UI change. Ship in any order; test in isolation.
- **Phase 5** introduces the new tab (skeleton only). Mesh tab visible but empty.
- **Phases 6-9** progressively add UI: canvas → table → drawer → wizard.
- **Phase 10** deletes Settings/MeshSection.tsx — first breaking change for the user.
- **Phase 11** is the bigger Settings reorganization.
- **Phases 12-14** are polish (activity, smart-add, animations).
- **Phase 15** removes the legacy Cron panel.
- **Phase 16** is docs.

Open items deferred from v1 (already in spec §10): WebSocket subscriptions for live updates beyond push events (currently Tauri events only), bundled daemon shipping, OpenClaw migration recipes, cross-mesh push routing, smart-add upgrade from heuristics to LLM-based agent worker.

**Spec coverage check:** Every section of `docs/superpowers/specs/2026-05-09-mesh-tab-design.md` maps to at least one task here. §3.4 (background probe scheduler) → Phase 4 Task 4.5. §3.3 (settings reorganization) → Phase 11. §4 (data model) → Phase 1 + 3 (schema, types, CRUD). §5 (REST + Tauri commands + CLI) → Phase 2. §6 (UI components) → Phases 5-9. §7 (mesh sync data flow) → Phase 6 useMeshData + Phase 12 useActivityLog. §8 (migration) → Phase 10 + 11 + 15. §9 (testing) → tests folded into each task.

**Method-signature consistency check:** `services_list`, `services_create`, `services_update`, `services_delete`, `services_regenerate_token`, `activity_list` are the canonical Tauri command names referenced in both backend (Phase 2 Task 2.2) and frontend (`meshApi.ts` Phase 5 Task 5.1) — names match. `validate_push` (Phase 3 Task 3.1) is consumed by `pane_routes.rs` modification in same task. `ActivityWriter::new(conn)` and `record(event)` consistent across Phase 3 Task 3.2 and Phase 14 Task 14.1.

---

**Plan complete and saved to `docs/superpowers/plans/2026-05-09-mesh-tab-implementation.md`.**

## Execution Handoff

Two execution options:

**1. Subagent-Driven (recommended for multi-week scope)** — Dispatch fresh subagent per task with two-stage review (implementation review + diff review). Best for protecting main context across the 40+ tasks here. Use `superpowers:subagent-driven-development`.

**2. Inline Execution** — Execute tasks in this session using `superpowers:executing-plans`. Faster turnaround per task but main context grows linearly. Best for the first 1-2 phases as a sanity check before handing off.

**Recommendation: subagent-driven**, given the multi-week scope and the goal of keeping each task's diff reviewable in isolation.
