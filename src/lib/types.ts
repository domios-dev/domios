import type { RemoteChannelAvailabilityMap } from "./remoteTabTransportState";

/** Title priority: higher levels cannot be overwritten by lower ones. */
export enum TitleLevel {
  Default = "Default",
  Auto = "Auto",
  Agent = "Agent",
  User = "User",
}

export interface Workspace {
  id: string;
  name: string;
  tabs: string[];
  active_tab_id: string | null;
  cwd: string | null;
  protected?: boolean;
  /** Sibling ordinal among workspaces; the sidebar sorts by it. */
  sort_order?: number;
}

/** A tab that was closed and can be reopened. Mirrors Rust `ClosedTab`
 *  (serde snake_case). `closed_at` is a UNIX-millisecond timestamp. Now backed
 *  by the sqlite closed-history store rather than the session snapshot. */
export interface ClosedTab {
  tab_id: string;
  /** Original parent tab id — preserves the orchestration hierarchy for a
   *  cascade-group reopen. */
  parent_tab_id?: string | null;
  title: string;
  description?: string | null;
  backing?: TabBacking;
  cwd?: string | null;
  agent_type?: string | null;
  agent_session_id?: string | null;
  model?: string | null;
  closed_at: number;
  /** Workspace the tab belonged to. Empty string on old records. */
  workspace_id?: string;
  workspace_name?: string;
}

/** A closed tab enriched for the sidebar: the per-workspace reopen `index`, an
 *  `orphaned` flag (its workspace no longer exists), and the group it belongs
 *  to. Mirrors Rust `ClosedTabView` (ClosedTab flattened + these fields). */
export interface ClosedTabView extends ClosedTab {
  index: number;
  orphaned: boolean;
  group_id: string;
  group_kind: "single" | "cascade" | "workspace";
}

export type AgentRole = "Manager" | "Worker";

export interface PromptOption {
  label: string;
  value: string;
}

export interface PromptSection {
  label: string;
  active?: boolean;
  completed?: boolean;
}

export interface PendingPrompt {
  question: string;
  options: PromptOption[];
  sections?: PromptSection[] | null;
  multi_select?: boolean | null;
  submit_available?: boolean | null;
  selected_values?: string[] | null;
}

/** What backs a tab's I/O — a PTY (terminal) or a plugin chatBackend. Mirrors
 *  the Rust `TabBacking` (serde `tag = "kind"`, snake_case). */
export type TabBacking =
  | { kind: "pty" }
  | { kind: "chat_backend"; plugin_id: string; session_id: string };

/** Tunable watcher limits set by the spawner; host clamps to safe bounds. */
export interface WatcherPolicy {
  tick_interval_secs: number;
  nudge_cooldown_secs: number;
  max_actions_per_tick: number;
}

/** Persisted watcher session state on a chatBackend tab. Mirrors Rust `WatcherSpec`. */
export interface WatcherSpec {
  charter: string;
  policy: WatcherPolicy;
  observations?: unknown;
  state?: unknown;
  tick?: number;
  last_tick_ms?: number | null;
  last_status?: string | null;
  tick_inflight_since_ms?: number | null;
}

/** Backend-computed live agent state. Mirrors the Rust `AgentLiveState` serde
 *  shape; `subtype` only carries meaning when `state === "question"`. */
export interface TabAgentState {
  state: "working" | "idle" | "question";
  /** Provider-rendered live status line, e.g. Claude's token/progress row. */
  status_line?: string | null;
  /** Provider-rendered completed-turn summary shown while idle. */
  last_turn_summary?: string | null;
  /** Complete authoritative input request shown while questioning. */
  question?: string | null;
  /** Provider-derived work phase, e.g. thinking/streaming/tool, when known. */
  phase?: string | null;
  /** Parsed elapsed time for the provider status line, in milliseconds. */
  elapsed_ms?: number | null;
  subtype?:
    | "model_choice"
    | "model_picker"
    | "permission"
    | "auth"
    | "rate_limit"
    | "update"
    | "error"
    | "unknown";
}

/** Truthful tab-boot progress, emitted by the daemon at real launch
 *  transitions (never timer-driven). Mirrors Rust `service::boot_stage::BootStage`. */
export type BootStage =
  | "pty_starting"
  | "pty_spawned"
  | "reader_ready"
  | "shell_ready"
  | "agent_launching"
  | "launch_written"
  | "provider_ready"
  | "preparing_role_instructions"
  | "delivering_instructions"
  | "agent_ready"
  | "pty_reattached"
  | "spawn_failed";

/** Payload of the `tab-boot-stage` event. */
export interface TabBootStageEvent {
  tab_id: string;
  stage: BootStage;
  reason?: string | null;
}

export interface Tab {
  id: string;
  workspace_id?: string | null;
  /** Per-group sibling ordinal within the owning workspace or hierarchy. */
  sort_order?: number;
  pty_id: string | null;
  /** Defaults to `{ kind: "pty" }` when absent (pre-field snapshots). */
  backing?: TabBacking;
  tab_type: "Terminal" | "Choice";
  title: string;
  description?: string | null;
  cwd: string | null;
  title_level?: TitleLevel;
  /** Frontend-only: true while `title` was set by a local optimistic rename
   *  not yet confirmed by a backend snapshot. Distinguishes a genuine
   *  in-flight rename (must survive a tied/stale backend snapshot) from a
   *  stale title merely carried over from a prior snapshot or cache
   *  hydration (must yield to the backend on the next tie). Cleared once a
   *  backend SET_STATE reports a matching title. */
  title_pending_local?: boolean;
  agent_type?: string | null;
  agent_session_id?: string | null;
  /** Launch-scoped proof used to reject chat events emitted by a prior tab binding. */
  session_binding_nonce?: string | null;
  agent_session_name?: string | null;
  agent_session_model?: string | null;
  reasoning_effort_identity?: {
    planned?: string | null;
    requested?: string | null;
    resolved?: string | null;
    actual: string | null;
    actual_source?: "session-owned" | "provider-default-unverified" | null;
  };
  context_epoch_archives?: Array<{
    provider: string;
    cwd: string;
    session_id: string;
  }>;
  role?: AgentRole | null;
  role_profile?: string | null;
  team_id?: string | null;
  spawned_by_tab_id?: string | null;
  parent_tab_id?: string | null;
  can_spawn?: boolean;
  depth?: number;
  chat_view?: boolean;
  pending_prompt?: PendingPrompt | null;
  /** Live 3-state agent classification from the backend state engine. Drives
   *  the chat-view AgentStateIndicator. Absent until the backend classifies. */
  agent_state?: TabAgentState | null;
  protected?: boolean;
  connected_services?: string[];
  hidden?: boolean;
  show_in_tabbar?: boolean;
  worker_id?: string | null;
  agent_booting?: boolean;
  /** True only when this visible session explicitly bypasses provider permissions/sandbox. */
  auto_approve?: boolean;
  /** Current agent-boot stage for late attachers reading the live snapshot.
   *  Absent once boot completes or before it begins. */
  boot_stage?: BootStage | null;
  boot_failure_reason?: string | null;
  /** Machine+watcher chatBackend session — read-only tab with tick-driven inputs. */
  watcher?: WatcherSpec | null;
  remote_peer_id?: string;
  remote_peer_name?: string;
  remote_tab_id?: string;
  remote_channels?: RemoteChannelAvailabilityMap;
  ownership?: RemoteOwnership;
}

export interface RemoteOwnership {
  controller_title: string;
  controller_tab_id?: string;
  peer_id: string;
  peer_name: string;
  team?: { id: string; title?: string };
}

export interface MultiplexerState {
  workspaces: Workspace[];
  tabs: Record<string, Tab>;
  teams?: Team[];
  active_workspace_id: string | null;
  focused_tab_id: string | null;
  /** Monotonic snapshot generation stamped by the backend. The reducer ignores
   *  any incoming snapshot whose generation is `<=` the one already applied. */
  generation?: number;
  /** Bounded newest-first view of the closed-tab history across all
   *  workspaces (sourced from the sqlite store, not the session snapshot).
   *  Each entry carries its per-workspace reopen `index`, an `orphaned` flag,
   *  and group metadata. */
  recently_closed?: ClosedTabView[];
  /** Aggregate Idle/Working/Waiting/Dead per PTY-backed tab, mirrored from the
   *  backend's live status tracker so the tab rail hydrates correctly on
   *  reload instead of staying blank until the next status event. */
  tab_statuses?: Record<string, TabStatus>;
}

export type TeamLifecycle = "forming" | "active" | "paused" | "complete" | "archived";

export interface Team {
  id: string;
  name?: string | null;
  lifecycle: TeamLifecycle;
  created_at: number;
  workspace_id: string;
  /** Set only on a synthesized remote Team; shows the origin peer once in the header. */
  remote_peer_name?: string | null;
}

/** Result of the one atomic spawn command (`mux_spawn_tab`). */
export interface AtomicSpawn {
  snapshot: MultiplexerState;
  tab_id: string;
  workspace_id: string;
  launch_command?: string;
  warning?: string;
}

export interface WorkspaceInfo {
  id: string;
  name: string;
  tab_count: number;
  is_active: boolean;
  cwd: string | null;
  protected?: boolean;
  tab_ids: string[];
  /** Set only when create_workspace deduped by cwd instead of creating one. */
  existing?: boolean;
}

export interface TabInfo {
  id: string;
  title: string;
  tab_id: string;
}

export interface PtyInfo {
  id: string;
  status: "running" | "dead";
}

export interface NotificationAction {
  label: string;
  /** "invoke" calls a Tauri command, "url" opens in browser, "dismiss" just closes */
  type: "invoke" | "url" | "dismiss";
  /** Tauri command name or URL */
  value?: string;
  /** Args for Tauri invoke */
  args?: Record<string, unknown>;
}

export interface Notification {
  id: string;
  key?: string;
  tab_id: string | null;
  workspace_id: string | null;
  title: string;
  body: string | null;
  icon_svg?: string | null;
  icon_color?: string | null;
  icon_hint?: string | null;
  level: "Info" | "Warning" | "Error";
  timestamp: number;
  read: boolean;
  /** Action buttons — if present, toast shows buttons instead of auto-dismiss */
  actions?: NotificationAction[];
  /** Persist toast — don't auto-dismiss, require explicit action */
  sticky?: boolean;
}

export type TabStatus = "Working" | "Waiting" | "Idle" | "Dead";

/** A worker running on another mesh peer that reports to a local orchestrator.
 *  It has no local PTY — the UI renders it as a read-only "remote Tab" ghost
 *  node under its orchestrator. Mirrors the Rust `RemoteWorker`. */
export interface RemoteWorker {
  /** Paired peer (instance name) the worker runs on. */
  peer: string;
  /** The worker's tab id on that peer. */
  remote_tab: string;
  title: string;
  model: string | null;
  /** Last reported lifecycle: working | done | blocked | partial. */
  status: string;
  last_message: string;
  last_seen_ms: number;
}

/** Map of local orchestrator tab id → its remote workers. */
export type RemoteWorkersByOrchestrator = Record<string, RemoteWorker[]>;

// Graph worker (Phase E memory visualization)

export type GraphWorkerStatus =
  | "queued"
  | "starting"
  | "reading"
  | "analyzing"
  | "writing"
  | "done"
  | "error"
  | "failed"
  | "cancelled"
  | "hard_timeout"
  | "timeout"
  | "timed_out"
  | "orphaned";

export interface GraphWorkerInfo {
  worker_id: string;
  tab_id: string | null;
  intent: string;
  spawned_by: string;
  spawned_at: number;
  status: GraphWorkerStatus;
  progress_text: string | null;
  touched_nodes: string[];
  color: string;
  report: string | null;
  done_at: number | null;
  log_path?: string | null;
  agent_provider?: string | null;
  model?: string | null;
  session_id?: string | null;
  cwd?: string | null;
}

export interface PluginActivity {
  id: string;
  pluginId: string;
  kind: string;
  label: string;
  percent?: number | null;
  status: "running" | "done" | "error";
  error?: string | null;
  startedAt: number;
  updatedAt: number;
}

export interface SpawnWorkerReq {
  intent: string;
  task: string;
  agent?: string;
  parent?: string;
}

// Cron types

export type CronRunStatus = "Running" | "Success" | "Failed" | "Cancelled";

export interface CronRunInfo {
  run_id: string;
  started_at: number;
  finished_at: number | null;
  status: CronRunStatus;
  tab_id: string | null;
  session_id?: string | null;
  cwd?: string | null;
}

export interface CronJob {
  id: string;
  name: string;
  description: string | null;
  workspace_id: string;
  team_id?: string | null;
  schedule: string;
  enabled: boolean;
  agent: string;
  /** Model id within the agent provider (e.g. "opus", "gpt-5.4-mini"). `null` = provider default. */
  model: string | null;
  task: string;
  cwd: string | null;
  role: string | null;
  creator_tab_id?: string | null;
  report_target_tab_id?: string | null;
  oneshot: boolean;
  max_history: number;
  created_at: number;
  last_run: CronRunInfo | null;
  next_run: number | null;
  /** Anti-predictability jitter cap in milliseconds. `null` = provider
   *  default (auto ~10% of interval), `0` = disabled, positive = max ms
   *  added to each scheduled fire. */
  jitter_max_ms: number | null;
}

// Configuration types (matches Rust AppConfig schema)

export interface ThemeConfig {
  name: string;
  background: string;
  foreground: string;
  cursor: string;
  selection: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  bright_black: string;
  bright_red: string;
  bright_green: string;
  bright_yellow: string;
  bright_blue: string;
  bright_magenta: string;
  bright_cyan: string;
  bright_white: string;
}

export interface FontConfig {
  family: string;
  size: number;
  ligatures: boolean;
}

export interface TerminalConfigSettings {
  cursor_style: string;
  cursor_blink: boolean;
  scrollback: number;
  copy_on_select: boolean;
}

export interface NotificationConfig {
  desktop: boolean;
  sound: boolean;
  toast_duration: number;
  telegram: TelegramNotifyConfig;
  /** The desktop gate's rows, generated by the daemon from the one closed
   *  category set. Settings render this list rather than restating it. */
  agent_categories: AgentCategoryChoice[];
}

/** One category the user may allow or refuse. An agent notification naming no
 *  category, or a category switched off here, never raises a desktop alert. */
export interface AgentCategoryChoice {
  category: string;
  enabled: boolean;
}

/** Opt-in push audience. Deliberately not `integrations.telegram_allowed_users`,
 *  which is an inbound admission list rather than an audience. */
export interface TelegramNotifyConfig {
  enabled: boolean;
  recipients: string[];
  per_scope_limit: number;
}

export interface KeybindingsConfig {
  reopen_tab?: string;
  /** Cycle to the next sidebar item (plan entry or tab) in the active workspace. */
  next_tab?: string;
  /** Cycle to the previous sidebar item (plan entry or tab) in the active workspace. */
  prev_tab?: string;
}

export interface SocketConfigSettings {
  path: string | null;
}

export interface AgentConfig {
  poll_interval_ms: number;
  idle_timeout_s: number;
  resume_sessions: boolean;
  full_access?: boolean;
}

export interface ApiConfig {
  enabled: boolean;
  host: string;
  port: number;
  token: string | null;
  cors: boolean;
  cors_allow_origins?: string[];
}

export interface GeneralAgentConfig {
  enabled: boolean;
  agent_provider: string;
  auto_restart: boolean;
  max_restart_attempts: number;
  restart_delay_s: number;
  first_run_complete?: boolean;
}

export interface IntegrationsConfig {
  telegram_enabled: boolean;
  telegram_allowed_users: string[];
  discord_enabled: boolean;
  discord_allowed_users: string[];
  webhook_enabled: boolean;
  webhook_port: number;
  secrets: SecretsConfig;
}

export interface SecretsConfig {
  backend: string;
  default_organization: string;
  default_collection: string;
  bitwarden?: BitwardenSettings;
}

export interface BitwardenSettings {
  server_url: string;
  persist_master_password: boolean;
}

export interface MeshConfig {
  enabled: boolean;
  instance_name: string;
  broadcast_duration_secs: number;
}

export interface OrchestrationConfig {
  plan_reminders: boolean;
  nudge_interval_secs: number;
}

export interface MeshStatus {
  enabled: boolean;
  instance_id: string;
  instance_name: string;
  discovery_active: boolean;
  broadcast_active: boolean;
  broadcast_expires_at: number | null;
  tailscale_ip: string | null;
  local_ip: string | null;
  api_port: number;
  paired_count: number;
  discovered_count: number;
}

export interface PeerAddress {
  ip: string;
  is_tailscale: boolean;
}

export interface PairedPeer {
  instance_id: string;
  instance_name: string;
  addresses: PeerAddress[];
  api_port: number;
  public_key: string;
  paired_at: string;
  last_seen: string | null;
  online: boolean;
  is_primary: boolean;
  access_level: string;
  version: string | null;
  web_ui?: WebUiAnnouncement | null;
  relay_protocol_version: number;
}

export const RELAY_PROTOCOL_VERSION = 1;

export type WebUiEndpoint =
  | { transport: "tailscale_https"; host: string; port: number }
  | { transport: "tailscale_http"; ip: string; port: number }
  | { transport: "lan_http"; ip: string; port: number };

export interface WebUiAnnouncement {
  protocol_version: number;
  endpoints: WebUiEndpoint[];
}

export interface DiscoveredPeer {
  instance_id: string;
  instance_name: string;
  addresses: PeerAddress[];
  api_port: number;
  discovery_method: "mdns" | "tailscale" | "udp" | "gossip" | "manual";
  last_seen: number;
  dev: boolean;
  mesh_id: string | null;
  /** instance_ids of paired peers who told us about this peer via gossip. */
  vouched_by: string[];
}

export interface MeshPairingRequest {
  request_id: string;
  from_instance_id: string;
  from_instance_name: string;
  from_address: string;
  from_port: number;
  timestamp: number;
  status: "pending" | "accepted" | "rejected" | "expired";
}

export interface MeshPeersResponse {
  paired: PairedPeer[];
  discovered: DiscoveredPeer[];
  incoming_requests: MeshPairingRequest[];
  local_instance_name: string;
}

/// Per-provider settings stored under `agents.<id>` in the TOML config.
/// Mirrors `src-tauri/src/config/schema.rs::ProviderConfig`.
export interface ProviderConfig {
  resume_sessions?: boolean;
  default_model?: string;
  /// When false, the provider is hidden from the per-workspace
  /// "New <Provider> Session" menu. Defaults to true (visible).
  visible_in_menu?: boolean;
  /// Periodic /usage probing for this provider. On by default; auto-rendered
  /// by `<SchemaRenderer />` from the plugin's `settings_schema()` when the
  /// provider declares a `usage_parser()`.
  usage_tracking_enabled?: boolean;
  // Additional plugin-declared fields land in this map (TOML #[serde(flatten)]).
  [key: string]: unknown;
}

export interface MemoryConfig {
  extraction_agent: string;
  extraction_model: string;
  auto_extract: boolean;
  dreaming_agent: string;
  dreaming_model: string;
  worker_model: string;
  worker_timeout_seconds: number;
  dedup_threshold: number;
  file_sync_enabled: boolean;
  file_sync_interval_minutes: number;
  librarian_enabled?: boolean;
}

export interface UiConfig {
  memory_tab_enabled?: boolean;
  mesh_tab_enabled?: boolean;
  keepawake_display?: boolean;
  usage_intro_hint_opens?: number;
  [key: string]: unknown;
}

export interface AppConfig {
  theme: ThemeConfig;
  font: FontConfig;
  terminal: TerminalConfigSettings;
  notifications: NotificationConfig;
  keybindings: KeybindingsConfig;
  socket: SocketConfigSettings;
  agent: AgentConfig;
  api: ApiConfig;
  agents: { [providerId: string]: ProviderConfig };
  general_agent: GeneralAgentConfig;
  memory: MemoryConfig;
  integrations: IntegrationsConfig;
  mesh: MeshConfig;
  orchestration: OrchestrationConfig;
  ui?: UiConfig;
}

// Provider descriptor wire types mirror `src-tauri/src/agent/provider.rs`.
// Frontend consumes them via `list_agent_providers`.

export interface ModelInfo {
  id: string;
  display_name: string;
  description?: string;
  available?: boolean | null;
}

export interface ReasoningEffortInfo {
  id: string;
  display_name: string;
  description?: string;
}

export interface PresetInfo {
  id: string;
  name: string;
  description?: string;
}

/** Runtime backing a provider — the spawn discriminant (was the `kind` string). */
export type ProviderTransport = "pty" | "chat_backend";

/** Behavior surface a provider exposes for configuration. Tagged on `model`. */
export type BehaviorConfig =
  | { model: "none" }
  | { model: "own_prompt_set" }
  | { model: "presets_and_system_prompt" }
  | { model: "graph" };

/** How a system prompt reaches the provider. */
export type SystemPromptDelivery =
  | { cli_flag: string }
  | "session_config"
  | "external";

export type NoteLevel = "info" | "warn" | "error";
export type BadgeStyle = "green" | "orange" | "red" | "gray";
export type ActionStyle = "primary" | "secondary" | "danger";
export type PathOpenKind = "open_folder" | "open_file" | "reveal_in_explorer";

export interface BadgeVariant {
  id: string;
  label: string;
  style: BadgeStyle;
}

/// Discriminated union — `kind` field tags the variant. Mirror of
/// `SettingField` enum (snake_case rename) on the Rust side.
export type SettingField =
  | { kind: "section"; title: string; description?: string; fields: SettingField[] }
  | { kind: "toggle"; key: string; label: string; default: boolean; description?: string }
  | { kind: "text"; key: string; label: string; default?: string; placeholder?: string; description?: string }
  | { kind: "number"; key: string; label: string; default?: number; min?: number; max?: number; description?: string }
  | { kind: "select"; key: string; label: string; options: [string, string][]; default?: string; description?: string }
  | {
      kind: "array";
      key: string;
      label: string;
      description?: string;
      item_label?: string;
      default?: Record<string, unknown>[];
      item: {
        kind: "object";
        fields: Array<
          | { kind: "text"; key: string; label: string; default?: string; placeholder?: string; description?: string }
          | { kind: "textarea"; key: string; label: string; default?: string; placeholder?: string; description?: string }
          | { kind: "number"; key: string; label: string; default?: number; min?: number; max?: number; description?: string }
          | { kind: "json"; key: string; label: string; default?: Record<string, unknown>; description?: string }
        >;
      };
    }
  | { kind: "api_key"; env_var: string; label: string; link?: string }
  | { kind: "info"; title?: string; body: string; level: NoteLevel }
  | { kind: "path_link"; label: string; path: string; open: PathOpenKind }
  | {
      kind: "badge";
      query_command: string;
      query_args?: Record<string, unknown>;
      variants: BadgeVariant[];
    }
  | {
      kind: "action";
      id: string;
      label: string;
      command: string;
      state_query?: string;
      args?: Record<string, unknown>;
      confirm?: string;
      style: ActionStyle;
    }
  | { kind: "when"; field: string; equals: unknown; then: SettingField[] };

export interface ProviderDescriptor {
  schema_version: number;
  id: string;
  display_name: string;
  /** Runtime backing — the spawn discriminant (replaces the old `kind` string). */
  transport: ProviderTransport;
  /** Behavior surface for configuration. Optional; defaults to `{ model: "none" }`. */
  behavior_config?: BehaviorConfig;
  /** Named presets (chat backends); agents default empty. */
  presets?: PresetInfo[];
  /** How a system prompt reaches the provider. */
  system_prompt_delivery: SystemPromptDelivery;
  icon_hint?: string;
  icon_svg?: string;
  icon_color?: string;
  available_models: ModelInfo[];
  default_model?: string;
  /// Reasoning-effort levels the provider supports at launch. Empty ⇒ the
  /// provider exposes no effort selector and the UI must not render one.
  reasoning_efforts?: ReasoningEffortInfo[];
  default_reasoning_effort?: string;
  settings_schema: SettingField[];
  /// Schema rendered into Settings → Notifications. Optional / may be empty
  /// (e.g. Codex contributes none today). Notifications tab iterates providers
  /// and renders one section per non-empty contribution.
  notifications_schema: SettingField[];
  supports_lazy_skills: boolean;
  has_hooks: boolean;
  supports_permission_bypass?: boolean;
  /// Provider can render a tab as chat (JSONL sessions or structured/DB-backed
  /// chat). Gates the chat/terminal toggle — replaces the old hardcoded set.
  supports_chat_ui?: boolean;
  /// Provider exposes a structured prompt source (claude AskUserQuestion in the
  /// JSONL transcript, opencode SSE bus) so pending prompts come from a handle
  /// rather than PTY scraping. Informational on the wire today.
  supports_structured_prompts?: boolean;
  /// Binary name to probe via `which` when checking installation. Optional —
  /// providers without a CLI dependency leave this null.
  cli_binary?: string | null;
  /// Shell command that installs the CLI for the current platform. Used by
  /// the Install card on the per-provider Settings screen.
  install_command?: string | null;
  /// Provider-declared in-agent model switch template. `{model}` is replaced
  /// with the selected ModelInfo id. Missing means the provider does not
  /// support arg-based live model switching.
  model_switch_format?: string | null;
  /// Provider-declared in-agent context-compaction command. Consumed
  /// capability-keyed by the orchestrator refresh cycle. Missing = no compact.
  compact_command?: string | null;
  self_update?: {
    inSession: boolean;
    restartRequired: boolean;
    completionSignal?: "provider_classification" | null;
  } | null;
  /// Optional `integration.trust_grant` capability: core invokes the
  /// provider-owned hook-trust grant once per integration install, scoped to
  /// these managed key(s). Informational on the wire.
  integration_trust_grant?: { managedKeys: string[] } | null;
}

/** Input for `createOrchestrationPair` — camelCase for ergonomic call sites;
 *  translated to the snake_case wire shape shared by CLI/REST/Tauri. */
export interface OrchestrationPairInput {
  orchestratorAgent: string;
  orchestratorModel?: string;
  plannerAgent: string;
  plannerModel?: string;
  workspaceId?: string;
  title?: string;
  /** Blank picker tab to claim for the Orchestrator in place (task_e0ddee08) —
   *  confirming the picker transforms that tab instead of leaving it behind. */
  existingTabId?: string;
}

/** Mirrors Rust `OrchestrationPairResult` (task_7e5f9354) — plain snake_case,
 *  same shape returned by the socket RPC, REST, and Tauri command. Success
 *  only: a failed Planner spawn rejects with the exact recovery command
 *  instead of resolving to a half-populated result. */
export interface OrchestrationPairResult {
  plan_id: string;
  orchestrator_tab_id: string;
  orchestrator_model?: string;
  planner_tab_id: string;
  planner_model?: string;
  warning?: string;
}
