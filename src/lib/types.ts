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
  tabs: Tab[];
  active_tab_id: string | null;
  cwd: string | null;
  protected?: boolean;
}

export interface Tab {
  id: string;
  title: string;
  description?: string | null;
  /** One pane per tab. Split layouts were removed. */
  pane_id: string;
}

export type AgentRole = "Orchestrator" | "Worker";

export interface PromptOption {
  label: string;
  value: string;
}

export interface PendingPrompt {
  question: string;
  options: PromptOption[];
}

/** What backs a pane's I/O — a PTY (terminal) or a plugin chatBackend. Mirrors
 *  the Rust `PaneBacking` (serde `tag = "kind"`, snake_case). */
export type PaneBacking =
  | { kind: "pty" }
  | { kind: "chat_backend"; plugin_id: string; session_id: string };

export interface Pane {
  id: string;
  pty_id: string | null;
  /** Defaults to `{ kind: "pty" }` when absent (pre-field snapshots). */
  backing?: PaneBacking;
  pane_type: "Terminal";
  title: string;
  description?: string | null;
  cwd: string | null;
  title_level?: TitleLevel;
  agent_type?: string | null;
  agent_session_id?: string | null;
  agent_session_name?: string | null;
  agent_session_model?: string | null;
  role?: AgentRole | null;
  parent_pane_id?: string | null;
  can_spawn?: boolean;
  depth?: number;
  chat_view?: boolean;
  pending_prompt?: PendingPrompt | null;
  protected?: boolean;
  connected_services?: string[];
  hidden?: boolean;
  show_in_tabbar?: boolean;
  worker_id?: string | null;
}

export interface MultiplexerState {
  workspaces: Workspace[];
  panes: Record<string, Pane>;
  active_workspace_id: string | null;
  focused_pane_id: string | null;
  /** Monotonic snapshot generation stamped by the backend. The reducer ignores
   *  any incoming snapshot whose generation is `<=` the one already applied. */
  generation?: number;
}

/** Result of the one atomic spawn command (`mux_spawn_pane`). */
export interface AtomicSpawn {
  snapshot: MultiplexerState;
  pane_id: string;
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
}

export interface TabInfo {
  id: string;
  title: string;
  pane_id: string;
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
  pane_id: string | null;
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

export type PaneStatus = "Working" | "Waiting" | "Idle" | "Dead";

// Graph worker (Phase E memory visualization)

export type GraphWorkerStatus =
  | "starting"
  | "reading"
  | "analyzing"
  | "writing"
  | "done"
  | "error"
  | "orphaned";

export interface GraphWorkerInfo {
  worker_id: string;
  pane_id: string;
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
  pane_id: string | null;
  tab_id: string | null;
  session_id?: string | null;
  cwd?: string | null;
}

export interface CronJob {
  id: string;
  name: string;
  description: string | null;
  workspace_id: string;
  schedule: string;
  enabled: boolean;
  agent: string;
  /** Model id within the agent provider (e.g. "opus", "gpt-5.4-mini"). `null` = provider default. */
  model: string | null;
  task: string;
  cwd: string | null;
  role: string | null;
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
}

export interface KeybindingsConfig {
  new_tab: string;
  close_tab: string;
  next_tab: string;
  prev_tab: string;
  close_pane: string;
  focus_pane_left: string;
  focus_pane_right: string;
  focus_pane_up: string;
  focus_pane_down: string;
  new_workspace: string;
  copy: string;
  paste: string;
  search: string;
  zoom_in: string;
  zoom_out: string;
  zoom_reset: string;
  open_settings: string;
}

export interface SocketConfigSettings {
  path: string | null;
}

export interface AgentConfig {
  poll_interval_ms: number;
  idle_timeout_s: number;
  resume_sessions: boolean;
  resume_skip_permissions: boolean;
}

export interface ApiConfig {
  enabled: boolean;
  host: string;
  port: number;
  token: string | null;
  cors: boolean;
}

export interface GeneralAgentConfig {
  enabled: boolean;
  agent_provider: string;
  auto_restart: boolean;
  max_restart_attempts: number;
  restart_delay_s: number;
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
}

/// Per-provider settings stored under `agents.<id>` in the TOML config.
/// Mirrors `src-tauri/src/config/schema.rs::ProviderConfig`.
export interface ProviderConfig {
  resume_sessions?: boolean;
  resume_skip_permissions?: boolean;
  default_model?: string;
  /// When false, the provider is hidden from the per-workspace
  /// "New <Provider> Session" menu. Defaults to true (visible).
  visible_in_menu?: boolean;
  /// Periodic /usage probing for this provider. Off by default; auto-rendered
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
  default_new_agent_pane_chat?: boolean;
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
  ui?: UiConfig;
}

// Provider descriptor wire types mirror `src-tauri/src/agent/provider.rs`.
// Frontend consumes them via `list_agent_providers`.

export interface ModelInfo {
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
  settings_schema: SettingField[];
  /// Schema rendered into Settings → Notifications. Optional / may be empty
  /// (e.g. Codex contributes none today). Notifications tab iterates providers
  /// and renders one section per non-empty contribution.
  notifications_schema: SettingField[];
  supports_lazy_skills: boolean;
  has_hooks: boolean;
  preserves_context_on_resume: boolean;
  /// Provider can render a pane as chat (JSONL sessions or structured/DB-backed
  /// chat). Gates the chat/terminal toggle — replaces the old hardcoded set.
  supports_chat_ui?: boolean;
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
}
