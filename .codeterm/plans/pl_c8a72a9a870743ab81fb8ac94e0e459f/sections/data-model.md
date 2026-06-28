**HOW the data model is typed and persisted.** One Rust type tree in `src-tauri/src/plan/types.rs` is the single contract every surface (CLI, REST, canvas) reads. A `Plan` owns a `Vec<Section>` (each with `subsections` — a real tree), a `Vec<Task>`, and tasks own `Vec<Milestone>`. The serde attributes carry the durability rules — defaulted fields so old JSON loads, `type`→`kind` rename, snake_case status enums:

```rust
pub struct Task {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")] pub kind: String,
    pub model: Option<String>,
    pub model_rationale: Option<String>,
    pub status: TaskStatus,                 // pending|in_progress|in_review|blocked|done|cancelled
    #[serde(default)] pub priority: TaskPriority,
    #[serde(default)] pub created_at: u64,
    #[serde(default)] pub updated_at: u64,
    #[serde(default)] pub status_changed_at: u64,
    #[serde(default)] pub section_ids: Vec<String>,   // many-to-many → sections
    #[serde(default)] pub body: String,
    #[serde(default)] pub parent_task_id: Option<String>,  // subtask nesting
    pub assigned_pane_id: Option<String>,
    // …tags, handle, lane, milestones, group
}
```

**Identity is stable across restart** via `session_id` over `pane_id` (`OrchestratorRef` carries both; the session id is the durable one). Every entity carries its own canvas position so layout overrides survive:

```rust
pub struct ElementCanvas { pub x: Option<f64>, pub y: Option<f64>, #[serde(default)] pub collapsed: bool }
```

Source of truth = one JSON per plan at `<project>/.codeterm/plans/<id>.json`, written atomically (temp-write + `rename`, see *Infra*). An in-memory index (`PlanStore`) holds parsed plans for fast cross-project listing; the daemon is the sole writer.

## Key decisions
- **`section_ids` many-to-many** — a task can cover several spec/plan sections; coverage (see *Coverage Model*) is derived from these edges, never stored.
- **`#[serde(default)]` everywhere new** — no migrations: a field added today loads yesterday's JSON as the default. Matches the project's "no backward-compat shims, breaking changes fine" rule for *shape* while staying forward-compatible on *load*.
- **Per-entity `ElementCanvas`** — position is data, so the deterministic auto-layout can still be overridden per node and the override persists.
