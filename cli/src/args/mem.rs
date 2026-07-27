use clap::Subcommand;

#[derive(Subcommand)]
pub enum MemAction {
    /// Search across mem_items (FTS5 + hybrid scoring)
    Search {
        /// Query text
        query: String,
        /// Filter by kind: skill | fact | note | conversation | script | peer | preference | daily | learning | rule | summary
        #[arg(long)]
        kind: Option<String>,
        /// Filter by area tag
        #[arg(long)]
        area: Option<String>,
        /// Restrict to global / workspace / workspace:<id>
        #[arg(long)]
        scope: Option<String>,
        /// Project path whose in-project plan index should be searched too
        #[arg(long)]
        project: Option<String>,
        /// Max results (default 5)
        #[arg(long, default_value = "5")]
        limit: usize,
        /// Run LLM rerank on top candidates (requires ANTHROPIC_API_KEY)
        #[arg(long)]
        rerank: bool,
    },
    /// Print one item by id
    Get {
        /// Mem item id (e.g. skill_home_assistant)
        id: String,
    },
    /// Delete an item by id
    Delete { id: String },
    /// Batch-delete every item in an area (use sparingly).
    Forget {
        /// Area name (e.g. auth, deploy, billing).
        area: String,
        /// Confirm the batch delete (refused without it).
        #[arg(long, alias = "confirm")]
        yes: bool,
    },
    /// Print the full audit trail for one item (create/update/promote/delete).
    History {
        /// Mem item id
        id: String,
    },
    /// Print {area -> {kind -> count}} for the store (or a single workspace).
    Taxonomy {
        /// Restrict to one workspace scope
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Show recent mem writes (newest first). With --follow, poll for new rows.
    Tail {
        /// Follow mode: poll every 500ms and print new rows as they appear.
        #[arg(long, short = 'f')]
        follow: bool,
        /// Max rows per fetch (default 20)
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Partial update of an existing mem item by id. Only the flags you
    /// pass are written; everything else stays as-is.
    Update {
        /// Mem item id
        id: String,
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        summary: Option<String>,
        #[arg(long)]
        area: Option<String>,
        /// Comma-separated tags (REPLACES existing tags)
        #[arg(long)]
        tags: Option<String>,
        /// New body content (re-indexes embedding). Use `-` to read stdin.
        /// REPLACES the whole body — to grow a body use `mem append`.
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        /// Read the new body from a file instead of --body (bypasses shell quoting).
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        /// New importance 1-5
        #[arg(long)]
        importance: Option<f64>,
        /// Actor label for audit trail (defaults to "user"). Used by automated workflows.
        #[arg(long, allow_hyphen_values = true)]
        changed_by: Option<String>,
    },
    /// Append text to an existing item's body (non-destructive). Loads the
    /// current body, appends with a blank-line separator, re-indexes the
    /// embedding. Use this instead of a full `update --body` replace.
    Append {
        /// Mem item id
        id: String,
        /// Text to append (positional). Use `-` for stdin, or pass --file.
        #[arg(allow_hyphen_values = true)]
        text: Option<String>,
        /// Read the text to append from a file instead (bypasses shell quoting).
        #[arg(long, value_name = "PATH")]
        file: Option<String>,
        /// Actor label for audit trail (defaults to "user").
        #[arg(long, allow_hyphen_values = true)]
        changed_by: Option<String>,
    },
    /// Restore a prior body from the audit history (undo a bad replace).
    /// With no --version, reverts to the body before the most recent change.
    Restore {
        /// Mem item id
        id: String,
        /// History row id (from `mem history <id>`) to restore the body of.
        /// Omit to undo the latest body change.
        #[arg(long)]
        version: Option<i64>,
        /// Actor label for audit trail (defaults to "user").
        #[arg(long, allow_hyphen_values = true)]
        changed_by: Option<String>,
    },
    /// Pin a mem item so it always loads in L1 wake-up.
    Pin {
        /// Mem item id (e.g. fact_my_thing)
        id: String,
    },
    /// Unpin a previously pinned item.
    Unpin { id: String },
    /// List all currently pinned items, ordered by importance/evidence/recency.
    Pinned {
        /// Max results (default 100, max 500)
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Save a new mem item
    Save {
        /// Item kind: skill | fact | note | conversation | script | peer | preference | daily | learning | rule | summary
        #[arg(long)]
        kind: String,
        /// Title (will be slugified into id). If omitted, first line of body.
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        /// Body content. Use `-` (or omit with piped stdin) to read stdin.
        #[arg(allow_hyphen_values = true)]
        body: Option<String>,
        /// Read body from a file instead of the positional arg (bypasses shell quoting).
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        /// Optional summary (~50-200 chars)
        #[arg(long, allow_hyphen_values = true)]
        summary: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Area tag (auth | deploy | home | ...)
        #[arg(long)]
        area: Option<String>,
        /// Importance 1-5 (default 3.0). Higher = more likely in L1 wake-up.
        #[arg(long, default_value = "3.0")]
        importance: f64,
        /// Scope: global (default) or workspace:<id>
        #[arg(long)]
        scope: Option<String>,
        /// Convenience: scope this item to workspace ID. Maps to scope="workspace:<ws>".
        #[arg(long)]
        workspace: Option<String>,
        /// Pin the item for L1 wake-up (always loaded).
        #[arg(long)]
        pin: bool,
        /// Path to source file on disk. Enables Librarian auto-sync.
        #[arg(long)]
        source_path: Option<String>,
        /// Overwrite an existing row with the same id (upsert). Use this to
        /// update the bundled identity placeholder or any pinned record.
        #[arg(long)]
        replace: bool,
        /// Skip auto-extraction worker for this save.
        #[arg(long)]
        skip_extract: bool,
    },
    /// Save a stable user/machine preference for wake-up context.
    #[clap(name = "save-preference")]
    SavePreference {
        /// Title (will be slugified into id). If omitted, first line of body.
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        /// Preference body. Use `-` (or omit with piped stdin) to read stdin.
        #[arg(allow_hyphen_values = true)]
        body: Option<String>,
        /// Read body from a file instead of the positional arg.
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        /// Optional summary (~50-200 chars)
        #[arg(long, allow_hyphen_values = true)]
        summary: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Area tag (planning | orchestration | machine | ...)
        #[arg(long)]
        area: Option<String>,
        /// Importance 1-5 (default 4.0). Higher = more likely in L1 wake-up.
        #[arg(long, default_value = "4.0")]
        importance: f64,
        /// Scope: global (default) or workspace:<id>
        #[arg(long)]
        scope: Option<String>,
        /// Convenience: scope this item to workspace ID. Maps to scope="workspace:<ws>".
        #[arg(long)]
        workspace: Option<String>,
        /// Pin the item for L1 wake-up (always loaded).
        #[arg(long)]
        pin: bool,
        /// Overwrite an existing row with the same id (upsert).
        #[arg(long)]
        replace: bool,
    },
    /// Print L0+L1 wake-up context (GA init injection)
    WakeUp {
        /// Total token budget (split L0 + L1)
        #[arg(long)]
        budget: Option<usize>,
        /// Identity layer only (~100 tokens)
        #[arg(long)]
        l0_only: bool,
        /// Skip identity layer (return L1 only)
        #[arg(long)]
        no_l0: bool,
        /// Append a per-item L1 score breakdown for debugging selection.
        #[arg(long)]
        explain: bool,
        /// Workspace override; defaults to the calling tab's workspace.
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Re-run extraction workers for items that haven't been extracted yet.
    /// Useful after LLM API failures, quota resets, or bulk imports with
    /// skip_extract. Workers fire sequentially with a short delay.
    #[clap(name = "extract-pending")]
    ExtractPending {
        /// Maximum number of items to queue (default 200).
        #[arg(long, default_value = "200")]
        limit: usize,
    },
    /// Skill management — folder-on-disk skills with mem index.
    /// Skills live as ~/.codeterm/skills/<name>/SKILL.md (+ optional support files).
    /// `add` registers the folder; Librarian re-vectorizes if SKILL.md changes.
    Skill {
        #[command(subcommand)]
        action: MemSkillAction,
    },
    /// Show mem store stats (counts by kind, blob sizes)
    Stats,
    /// Capture a quick learning (insight, error, feature request)
    Learn {
        /// What you learned. If omitted and stdin piped, read stdin.
        #[arg(allow_hyphen_values = true)]
        text: Option<String>,
        /// "insight" (default), "error", or "feature"
        #[arg(long)]
        kind: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Shortcut: --error <text> = --kind error
        #[arg(long, allow_hyphen_values = true)]
        error: Option<String>,
        /// Shortcut: --feature <text> = --kind feature
        #[arg(long, allow_hyphen_values = true)]
        feature: Option<String>,
    },
    /// List pending learnings (kind=learning items)
    LearnList,
    /// Promote a learning to a fact (raises importance)
    LearnPromote { id: String },
    /// Flush a tab's PTY scrollback as a kind=conversation mem_item
    /// (called by PreCompact hook before context summarization).
    Flush {
        /// Tab id (default: $CODETERM_TAB_ID)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Optional title hint
        #[arg(long)]
        title: Option<String>,
    },
    /// Trigger file sync for source-tracked items (Librarian)
    Sync {
        /// Sync one file by path
        #[arg(long)]
        path: Option<String>,
    },
    /// Graph operations
    Graph {
        #[command(subcommand)]
        action: MemGraphAction,
    },
    /// Re-embed mem_items and/or graph entities into mem_vectors. Use this
    /// after swapping the embedder model, or to backfill existing entities
    /// that pre-date the auto-embed-on-create wiring. Daemon-routed (the
    /// daemon owns the embedder process). Idempotent.
    Reembed {
        /// What to re-embed: items (mem_items) | entities (graph) | all.
        #[arg(long, default_value = "all")]
        scope: String,
    },
    /// Graph-worker operations (hidden mem panel agents)
    Worker {
        #[command(subcommand)]
        action: MemWorkerAction,
    },
    /// Run a single mem maintenance pass in this subprocess. Opens the DB
    /// directly (multi-process WAL) and performs one phase of work, then
    /// exits. Used by the daemon's supervisor so a panic / OOM in dreaming
    /// or librarian cannot take the API down.
    #[clap(name = "run-worker")]
    RunWorker {
        /// Which phase to run: dreaming-light | dreaming-deep | dreaming-normalize | librarian-once
        kind: String,
        /// Override the path to knowledge.db. Defaults to the same data dir
        /// the CLI uses elsewhere (`~/.codeterm/mem/knowledge.db` for prod,
        /// `~/.codeterm-dev/mem/knowledge.db` for dev).
        #[arg(long)]
        db_path: Option<String>,
    },
    /// Pluggable secret store: save / get / search / list / delete.
    /// Backend is selected in Settings (default: bitwarden once shipped;
    /// `mock` in Phase 1 builds).
    Secret {
        #[command(subcommand)]
        action: MemSecretAction,
    },
    /// Dreamer review/apply: run a daemon-routed dream phase, or apply a
    /// reviewed re-cluster plan against the local mem DB.
    Dream {
        #[command(subcommand)]
        action: MemDreamAction,
    },
    /// Memory calibration: review or trigger the dreamer's importance/archive sweep.
    Calibration {
        #[command(subcommand)]
        action: MemCalibrationAction,
    },
}

#[derive(Subcommand)]
pub enum MemCalibrationAction {
    /// Show recent calibration changes (audit log).
    Log {
        /// ISO8601 lower-bound for changed_at (default: epoch)
        #[arg(long)]
        since: Option<String>,
        /// Max rows (default 50, max 500)
        #[arg(long)]
        limit: Option<usize>,
        /// Return just the count (no rows)
        #[arg(long)]
        count: bool,
    },
    /// Manually trigger a calibration sweep (Sunday only unless --force).
    Sweep {
        /// Bypass the day-of-week gate. Manual / testing.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum MemDreamAction {
    /// Run dreaming consolidation via the daemon. Phases: light, deep,
    /// normalize, all (default). Replaces the v1.1.22 `mem dream --phase ...`
    /// form which is gone in v1.1.23.
    Run {
        /// Which phase to run: light | deep | normalize | all
        #[arg(long, default_value = "all")]
        phase: String,
    },
    /// Re-parse a plan file and call `introduce_topic` for each YAML-fenced
    /// action block. Malformed individual fences become warnings; the rest
    /// still run. After a successful run the file is renamed to
    /// `<name>-applied.md` with a summary footer appended. Plans are
    /// hand-written or produced by a future Dreamer re-cluster phase.
    Apply {
        /// Path to the plan markdown file.
        plan_file: String,
        /// Override the path to knowledge.db.
        #[arg(long)]
        db_path: Option<String>,
    },
    /// Show the most recent dream diary entries (the structured PATTERN /
    /// CONNECTION / IDEA / WHAT IF / HUBS / ONE ACTION reflections the REM
    /// agent writes after each dream cycle). Use this to read the Dreamer's
    /// fresh observations before acting on them.
    Diary {
        /// How many entries to return (most-recent first).
        #[arg(long, default_value_t = 1)]
        limit: usize,
    },
}

#[derive(Subcommand)]
pub enum MemSecretAction {
    /// Save a secret. Value comes from --value or stdin (preferred — keeps
    /// the value out of shell history).
    Save {
        #[arg(long)]
        name: String,
        /// Plaintext value. Omit and pipe via stdin instead.
        #[arg(long)]
        value: Option<String>,
        #[arg(long)]
        folder: Option<String>,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        collection: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        /// Allow replacing an existing secret with the same name.
        #[arg(long)]
        overwrite: bool,
    },
    /// Resolve and print a secret. Default = raw value to stdout (no
    /// trailing newline). Use --json for {name, value, notes}.
    Get {
        #[arg(long)]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Free-text search over saved secret metadata.
    Search { query: String },
    /// List saved secrets (metadata only — never values).
    List {
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        folder: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Delete by name.
    Delete {
        #[arg(long)]
        name: String,
        #[arg(long)]
        yes: bool,
    },
    /// Show backend + auth status.
    Status,
    /// Pull latest from the remote vault. No-op for backends with no
    /// remote (Local OS-keychain, Mock). Bitwarden runs `bw sync`.
    Sync,
    /// Unlock the active backend with a master password (read from stdin if
    /// --password is omitted, never from argv).
    Unlock {
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        password: Option<String>,
    },
    /// Lock the active backend (in-process session is dropped).
    Lock,
    /// Sign out — also drops persisted creds (OS keychain entry).
    Logout,
    /// Save the master password to ~/.codeterm/secrets/bitwarden-master.env
    /// (mode 0600). Daemon then auto-unlocks JIT on every operation — no
    /// further prompts. Reads value from stdin (preferred — out of shell
    /// history) or --password.
    SaveMasterPassword {
        #[arg(long)]
        password: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum MemWorkerAction {
    /// Spawn a new graph worker
    Spawn {
        #[arg(long)]
        intent: String,
        #[arg(long)]
        task: String,
        #[arg(long, default_value = "claude")]
        agent: String,
        #[arg(long)]
        parent: Option<String>,
    },
    /// List all graph workers
    List,
    /// Get one worker by id
    Get { id: String },
    /// Report progress (called by worker itself; uses $CODETERM_WORKER_ID)
    Progress {
        text: String,
        #[arg(long)]
        status: Option<String>,
    },
    /// Submit final report and mark done (uses $CODETERM_WORKER_ID)
    Report { summary: String },
    /// Kill a worker by id
    Kill { id: String },
}

#[derive(Subcommand)]
pub enum MemSkillAction {
    /// Scaffold a new skill folder under ~/.codeterm/skills/<name>/, write
    /// a SKILL.md template (with frontmatter + canonical sections) plus an
    /// empty scripts/ subdir, then register it in mem immediately. After
    /// this, edit the SKILL.md — Librarian will re-index on change.
    New {
        /// Skill slug (also folder name). Use kebab-case.
        name: String,
        /// Optional area tag to put in the frontmatter.
        #[arg(long)]
        area: Option<String>,
    },
    /// Register an existing skill folder. Reads <folder>/SKILL.md and saves
    /// a mem item with source_path linked to it. Re-runs are idempotent.
    /// Area can come from --area flag, YAML frontmatter (`area: foo`), or
    /// the parent folder name (~/.codeterm/skills/<area>/<skill>/SKILL.md).
    Add {
        /// Folder containing SKILL.md (e.g. ~/.codeterm/skills/mem-testing).
        folder: String,
        /// Override the auto-detected area.
        #[arg(long)]
        area: Option<String>,
        /// Importance 1-5, default 4.
        #[arg(long, default_value = "4")]
        importance: f64,
    },
    /// List all registered skills.
    List,
}

#[derive(Subcommand)]
pub enum MemGraphAction {
    /// Create a graph entity
    Create {
        /// Entity id (e.g. person_kai)
        #[arg(long)]
        id: String,
        /// Entity type (Person, Project, Task, Skill, ...)
        #[arg(long, name = "type")]
        entity_type: String,
        /// JSON properties
        #[arg(long, default_value = "{}")]
        props: String,
        #[arg(long)]
        scope: Option<String>,
    },
    /// Create a relation between entities or mem_items
    Relate {
        #[arg(long)]
        from: String,
        #[arg(long)]
        rel: String,
        #[arg(long)]
        to: String,
        /// Validity start (ISO 8601, default now)
        #[arg(long)]
        valid_from: Option<String>,
        /// Validity end (ISO 8601, optional)
        #[arg(long)]
        valid_to: Option<String>,
        #[arg(long, default_value = "1.0")]
        confidence: f64,
        #[arg(long)]
        scope: Option<String>,
        /// Free-text reason; written to relations.reason for audit.
        #[arg(long)]
        reason: Option<String>,
    },
    /// List outgoing relations from an id
    Related {
        #[arg(long)]
        id: String,
        /// Filter by relation type
        #[arg(long)]
        rel: Option<String>,
        /// As-of date (ISO 8601) — filters by valid_from/valid_to
        #[arg(long)]
        as_of: Option<String>,
        /// Include relations whose valid_to is set (default: active-only)
        #[arg(long)]
        include_invalidated: bool,
    },
    /// Invalidate a relation (set valid_to, preserve history)
    Invalidate {
        #[arg(long)]
        from: String,
        #[arg(long)]
        rel: String,
        #[arg(long)]
        to: String,
        /// When it ended (ISO 8601, default now)
        #[arg(long)]
        ended: Option<String>,
    },
    /// Full temporal timeline for an entity
    Timeline {
        #[arg(long)]
        entity: String,
        /// Include relations whose valid_to is set (default: active-only)
        #[arg(long)]
        include_invalidated: bool,
    },
    /// BFS traversal from an entity (max_hops cap)
    Traverse {
        #[arg(long)]
        from: String,
        #[arg(long, default_value = "2")]
        hops: usize,
        #[arg(long, default_value = "100")]
        max_nodes: usize,
        #[arg(long)]
        as_of: Option<String>,
        /// Include relations whose valid_to is set (default: active-only)
        #[arg(long)]
        include_invalidated: bool,
    },
    /// Shortest path between two entities
    Path {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, default_value = "4")]
        max_hops: usize,
        /// Include relations whose valid_to is set (default: active-only)
        #[arg(long)]
        include_invalidated: bool,
    },
    /// List entities bridging 2+ areas (cross-project connections)
    Tunnels {
        /// Include relations whose valid_to is set (default: active-only)
        #[arg(long)]
        include_invalidated: bool,
    },
    /// Inspect a graph entity: properties_json + degree summary. Filling the
    /// gap that mem search only hits mem_items, not entities. Use this when
    /// you need email/phone/role/whatever a Person/Tech/Project entity carries
    /// in its properties — `mem graph related` returns relations only.
    Get {
        /// Entity id (e.g. person_andrey, tech_codeterm).
        id: String,
    },
    /// List hub nodes — entities with active degree at or above threshold.
    /// Each hub returns its degree, the relation_type distribution, and a
    /// sample of neighbors with titles. Used by the Dreamer recluster phase
    /// to decide which hubs to decompose into topic_* intermediates.
    Hubs {
        #[arg(long, default_value_t = 20)]
        threshold: u32,
        /// Per-hub neighbor sample size (default 30).
        #[arg(long, default_value_t = 30)]
        sample: u32,
    },
    /// List connected components of size <= max-size — disconnected
    /// "islands" the Dreamer should bridge or delete. Returns component
    /// member id/kind/title/area, plus a `suggested_anchor` derived from
    /// shared area / kind / title-keywords across members. JSON mode emits
    /// JSONL (one component per line, sorted by size asc).
    Islands {
        /// Maximum component size to include (default 6 — pairs through
        /// small clusters; bigger groups are real sub-graphs).
        #[arg(long, default_value_t = 6)]
        max_size: usize,
        /// Maximum number of components to emit (caps verbose output).
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// List nodes with degree 0 — true orphans (entities with no relations,
    /// mem_items not pointed at by anything). Output: id, kind, title,
    /// area, last access. JSON mode emits a flat array.
    Orphans {
        /// Cap the number of orphans emitted.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Merge one entity into another (alias → canonical)
    Alias {
        #[arg(long)]
        alias: String,
        #[arg(long)]
        canonical: String,
    },
    /// Atomically re-route an active direct edge (from → to) through an
    /// intermediate (from → via → to). Invalidates the original direct edge.
    Route {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        via: String,
        /// Relation type for the from→via segment (default = original rel).
        #[arg(long = "rel-in")]
        rel_in: Option<String>,
        /// Relation type for the via→to segment (default = original rel).
        #[arg(long = "rel-out")]
        rel_out: Option<String>,
        /// Free-text reason; written to relations.reason.
        #[arg(long)]
        reason: Option<String>,
        /// Override the path to knowledge.db.
        #[arg(long)]
        db_path: Option<String>,
    },
    /// Hard-delete relations whose valid_to is set. Optional filters by
    /// cutoff date and reason substring. Audited to graph_purge_log.
    #[clap(name = "purge-invalidated")]
    PurgeInvalidated {
        /// Only purge rows with valid_to before this RFC 3339 timestamp.
        #[arg(long)]
        before: Option<String>,
        /// Match relations.reason via LIKE %substring%.
        #[arg(long)]
        reason: Option<String>,
        /// Print count without modifying any rows or writing audit row.
        #[arg(long)]
        dry_run: bool,
        /// Override the path to knowledge.db.
        #[arg(long)]
        db_path: Option<String>,
    },
    /// Restore valid_to=NULL for relations whose endpoints both still exist
    /// in entities or mem_items. Idempotent.
    #[clap(name = "revive-broken")]
    ReviveBroken {
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        dry_run: bool,
        /// Override the path to knowledge.db.
        #[arg(long)]
        db_path: Option<String>,
    },
    /// Atomically introduce a topic intermediate: insert topic entity (if
    /// missing), link sources → topic → anchor, invalidate direct
    /// sources → anchor edges.
    #[clap(name = "introduce-topic")]
    IntroduceTopic {
        #[arg(long = "topic-id")]
        topic_id: String,
        #[arg(long = "type")]
        entity_type: String,
        /// Comma-separated source ids (s1,s2,s3) OR @path/to/file (one per
        /// line; '#'-prefixed and blank lines ignored).
        #[arg(long)]
        sources: String,
        #[arg(long)]
        anchor: String,
        #[arg(long = "rel-source", default_value = "about")]
        rel_source: String,
        #[arg(long = "rel-anchor", default_value = "about")]
        rel_anchor: String,
        #[arg(long)]
        reason: Option<String>,
        /// Override the path to knowledge.db.
        #[arg(long)]
        db_path: Option<String>,
    },
    /// Find duplicate relations grouped by selected columns.
    /// Read-only diagnostic — does not mutate the graph.
    #[clap(name = "find-duplicate-relations")]
    FindDuplicateRelations {
        /// Comma-separated columns from: from_id,relation_type,to_id,scope,valid_from.
        /// Default excludes valid_from because the relations UNIQUE(from,rel,to,valid_from)
        /// makes grouping by all four a guaranteed no-op — meaningful dedup is on the triple.
        #[arg(long, default_value = "from_id,relation_type,to_id")]
        group_by: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Delete relations by explicit rowids (audited to graph_purge_log).
    #[clap(name = "delete-relations")]
    DeleteRelations {
        /// Comma-separated rowids (e.g. "42,87,103")
        #[arg(long)]
        rowids: String,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Merge relations into `keep`, folding properties/confidence.
    #[clap(name = "merge-relations")]
    MergeRelations {
        /// Rowid of the relation to keep (its properties and rowid survive).
        #[arg(long)]
        keep: i64,
        /// Comma-separated rowids to fold into `keep` (these are deleted).
        #[arg(long)]
        merge: String,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Find duplicate entities (exact name match, alias chain, or vector similarity).
    #[clap(name = "find-duplicate-entities")]
    FindDuplicateEntities {
        /// Strategy: exact-name | alias-chain | vector
        #[arg(long, default_value = "exact-name")]
        strategy: String,
        /// Cosine similarity threshold for `vector` strategy (0.0-1.0).
        /// 0.90+ = strong-merge candidate, 0.85+ = review-worthy.
        #[arg(long, default_value_t = 0.85)]
        threshold: f64,
        /// Restrict `vector` strategy to entities of this type (Person, Tech, Project, ...).
        #[arg(long)]
        entity_type: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Patch properties on an existing entity (audit-logged).
    #[clap(name = "augment-entity")]
    AugmentEntity {
        /// Entity id (e.g. person_andrey)
        #[arg(long)]
        id: String,
        /// JSON object of properties to merge in (e.g. '{"city":"Riga"}')
        #[arg(long)]
        patch: String,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Find and (optionally) delete orphan rows in mem_vectors — vectors whose
    /// mem_id has no matching mem_items / entities row. One-shot legacy cleanup.
    #[clap(name = "repair-orphan-vectors")]
    RepairOrphanVectors {
        /// Print orphans without deleting anything.
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Find and (optionally) delete vectors whose mem_item has no real body
    /// (empty / title-only) — the "bodyless embeddings" that pollute search.
    #[clap(name = "prune-thin-vectors")]
    PruneThinVectors {
        /// Print the thin-body vector ids without deleting anything.
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Collapse parallel membership-synonym edges graph-wide in one pass: per
    /// node-pair keep ONE of about/applies_to/belongs_to/part_of/documented_in,
    /// invalidate the rest. Distinct predicates (uses, runs_on, …) untouched.
    #[clap(name = "collapse-synonym-edges")]
    CollapseSynonymEdges {
        /// Print the redundant edge rowids without invalidating anything.
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Invalidate dream-diary backlinks into hubs in one pass: every edge from a
    /// daily diary to a graph entity via a noise predicate (keeps the dreams anchor).
    #[clap(name = "prune-dream-hub-edges")]
    PruneDreamHubEdges {
        /// Print the dream backlink rowids without invalidating anything.
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Find and (optionally) delete sha256_*.txt blob files in the mem blobs
    /// directory that no mem_item references. CAS-aware sweep (refcount = 0).
    #[clap(name = "repair-orphan-blobs")]
    RepairOrphanBlobs {
        /// Print orphans without deleting anything.
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Delete an entity by id (audited). Refuses if it has active relations
    /// unless --force, in which case those relations are cascade-deleted too.
    #[clap(name = "delete-entity")]
    DeleteEntity {
        /// Entity id (e.g. seed_person_anna)
        #[arg(long)]
        id: String,
        /// Cascade-delete active relations referencing this entity.
        #[arg(long)]
        force: bool,
        /// Also hard-delete invalidated (historical) relations referencing this entity.
        /// Default false: history is preserved as ghost rows for `revive-broken` and
        /// timeline queries. Set this when you're sure the entity is gone for good.
        #[arg(long)]
        purge_history: bool,
        #[arg(long, default_value = "user")]
        actor: String,
    },
}
