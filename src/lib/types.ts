import type { AutoConfirmedPrompt } from "../../gen/AutoConfirmedPrompt";
import type { PendingPrompt } from "../../gen/PendingPrompt";
import type { BootStage } from "../../gen/BootStage";
import type { ProvisionalLaunch } from "../../gen/ProvisionalLaunch";
import type { RemoteOwnership } from "../../gen/RemoteOwnership";
import type { TabStatus } from "../../gen/TabStatus";
import type { AgentRole } from "../../gen/AgentRole";
export type { AgentRole } from "../../gen/AgentRole";
export type { PromptOption } from "../../gen/PromptOption";
export type { PromptSection } from "../../gen/PromptSection";
export type { AutoConfirmedPrompt } from "../../gen/AutoConfirmedPrompt";
export type { PendingPrompt } from "../../gen/PendingPrompt";
export type { BootStage } from "../../gen/BootStage";
export type { ProvisionalLaunch } from "../../gen/ProvisionalLaunch";
export type { RemoteOwnership } from "../../gen/RemoteOwnership";
export type { TabStatus } from "../../gen/TabStatus";
export type { TeamLifecycle } from "../../gen/TeamLifecycle";
export type { ThemeConfig } from "../../gen/ThemeConfig";
export type { FontConfig } from "../../gen/FontConfig";
export type { TerminalConfig as TerminalConfigSettings } from "../../gen/TerminalConfig";
export type { DiagnosticsConfig as DiagnosticsConfigSettings } from "../../gen/DiagnosticsConfig";
export type NotificationConfig = Omit<import("../../gen/NotificationConfig").NotificationConfig, "agent_levels"> & { agent_levels: AgentLevelChoice[] };
type NamedAgentLevelChoice = Pick<import("../../gen/AgentLevelChoice").AgentLevelChoice, "level" | "bell" | "desktop" | "sound">;
export type AgentLevelChoice = NamedAgentLevelChoice & Record<string, NamedAgentLevelChoice[keyof NamedAgentLevelChoice]>;
export type { PushChannelConfig } from "../../gen/PushChannelConfig";
export type { KeybindingsConfig } from "../../gen/KeybindingsConfig";
export type { SocketConfig as SocketConfigSettings } from "../../gen/SocketConfig";
export type { AgentConfig } from "../../gen/AgentConfig";
export type { GeneralAgentConfig } from "../../gen/GeneralAgentConfig";
export type { IntegrationsConfig } from "../../gen/IntegrationsConfig";
export type { SecretsConfig } from "../../gen/SecretsConfig";
export type { BitwardenSettings } from "../../gen/BitwardenSettings";
export type { MeshConfig } from "../../gen/MeshConfig";
export type { OrchestrationConfig } from "../../gen/OrchestrationConfig";
export type { ProviderConfig } from "./configWireFallbacks";
export type { SupporterSources } from "../../gen/SupporterSources";
export type { MemoryRerankConfig } from "../../gen/MemoryRerankConfig";
export type { SupporterConfig } from "../../gen/SupporterConfig";
export type { MemoryConfig } from "../../gen/MemoryConfig";
export type { UiConfig } from "./configWireFallbacks";
export type { ReplacedModelDefault } from "../../gen/ReplacedModelDefault";
export type { ModelDefaultsConfig } from "../../gen/ModelDefaultsConfig";
export type ApiConfig = import("../../gen/ApiConfig").ApiConfig & { token: string | null };
export type AppConfig = Omit<import("../../gen/AppConfig").AppConfig, "api" | "notifications"> & { api: ApiConfig; notifications: NotificationConfig };
import type { RuntimeServiceTierSelection as ServiceTierSelection } from "../../packages/plugin-sdk/src/agentRuntime";
import type { RemoteChannelAvailabilityMap } from "./remoteTabTransportState";
import type {ProviderSignals} from "./providerSignals";

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
  cwd_missing?: boolean;
  protected?: boolean;
  /** Over the plan limit: visible but not usable until the plan allows it. */
  locked?: boolean;
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

export interface ClosedStructureView {
  id: string;
  state: "closed" | "restoring" | "restored";
  kind: "team" | "workspace";
  label: string;
  closed_at: number;
  undo_until: number;
  member_count: number;
  team_count: number;
  shell_count: number;
  agent_count: number;
  cron_count: number;
  blueprint_count: number;
}











/** A plugin surface tab: a plugin view rendered inside a core-owned tab frame. */
export interface SurfaceTabBacking {
  kind: "surface";
  plugin_id: string;
  surface_id: string;
  instance_id: string;
  /** The tab is labelled after its owner (`parent_tab_id`). */
  owner_label?: boolean;
}

/** What backs a tab's I/O — a PTY (terminal), a plugin chatBackend, or a plugin
 *  surface. Mirrors the Rust `TabBacking` (serde `tag = "kind"`, snake_case). */
export type TabBacking =
  | { kind: "pty" }
  | { kind: "chat_backend"; plugin_id: string; session_id: string }
  | SurfaceTabBacking;

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
  /** Parsed elapsed time in milliseconds: the live turn while working, the finished turn while idle. */
  elapsed_ms?: number | null;
  subtype?: "model_choice" | "model_picker" | "permission" | "auth" | "rate_limit" | "update" | "error" | "unknown";
}



/** Payload of the `tab-boot-stage` event. */
export interface TabBootStageEvent {
  tab_id: string;
  stage: BootStage;
  agent_booting?: boolean;
  session_bound?: boolean;
  reason?: string | null;
  provisional_launch?: ProvisionalLaunch | null;
  launch_generation?: {
    epoch: number | null;
    operation_id: string | null;
    pty_id: string | null;
    binding_nonce: string | null;
  };
}



export interface Tab {
  agent_runtime_config?: { confirmedServiceTier?: { selection: ServiceTierSelection; sessionId: string; bindingNonce: string | null }; serviceTier?: ServiceTierSelection; serviceTierAvailability?: Record<string, readonly import("../../packages/plugin-sdk/src/agentRuntime").RuntimeServiceTier[]>; serviceTierAvailabilityBinding?: {sessionId:string;bindingNonce:string|null}; [key: string]: unknown } | null;
  agent_runtime_questions?: import("./agentRuntimeQuestions").AgentRuntimeQuestion[];
  agent_runtime_state?: import("./agentRuntimeState").AgentRuntimeState | null;
  pending_model_selection?: { model: string; effort: string | null; confirmation_pending?: boolean } | null;
  pending_model_switch_observation?: { launched_model: string } | null;
  close_restore?: { command: string | null; stopped: boolean; record_id: string | null; exact_session: boolean } | null;
  shell_profile?: ShellProfile | null;
  purpose?: import("./shellCommandState").ShellPurpose;
  icon?: string | null;
  shell_command_state?: import("./shellCommandState").ShellCommandState;
  context?: { used_tokens: number; window_tokens: number; cost_usd: number | null; source: string };
  provider_signals?: ProviderSignals;
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
  reported_session_binding_nonce?: string | null;
  agent_session_name?: string | null;
  agent_session_model?: string | null;
  agent_requested_model?: string | null;
  structured_runtime_mode?: string | null;
  agent_provider_resolved_model?: string | null;
  agent_label?: string | null;
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
  roleless_tracked?: boolean;
  /** The tab's own checklist currently holds items; peers report it for their tabs too. */
  has_checklist?: boolean;
  team_id?: string | null;
  spawned_by_tab_id?: string | null;
  managed?: { manager_tab_id: string; manager_name: string; manager_closed: boolean } | null;
  parent_tab_id?: string | null;
  can_spawn?: boolean;
  depth?: number;
  chat_view?: boolean;
  pending_prompt?: PendingPrompt | null;
  pending_prompt_kind?: import("./notificationTypes").NotificationPromptIdentity["kind"] | null;
  /** Live 3-state agent classification from the backend state engine. Drives
   *  the chat-view AgentStateIndicator. Absent until the backend classifies. */
  agent_state?: TabAgentState | null;
  protected?: boolean;
  /** Over the plan limit: visible but takes no input until the plan allows it. */
  locked?: boolean;
  connected_services?: string[];
  hidden?: boolean;
  show_in_tabbar?: boolean;
  worker_id?: string | null;
  agent_booting?: boolean;
  frozen?: boolean;
  slept_at_ms?: number | null;
  last_active_at_ms?: number | null;
  /** True only when this visible session explicitly bypasses provider permissions/sandbox. */
  auto_approve?: boolean;
  /** Opt-in: provider-recognized permission prompts are answered with their affirmative option. */
  permission_auto_confirm?: boolean;
  permission_auto_confirmations?: AutoConfirmedPrompt[];
  /** Current agent-boot stage for late attachers reading the live snapshot.
   *  Absent once boot completes or before it begins. */
  boot_stage?: BootStage | null;
  lifecycle_epoch?: number | null;
  lifecycle_operation_id?: string | null;
  session_bound?: boolean;
  boot_failure_reason?: string | null;
  provisional_launch?: ProvisionalLaunch | null;
  /** Machine+watcher chatBackend session — read-only tab with tick-driven inputs. */
  watcher?: WatcherSpec | null;
  remote_peer_id?: string;
  remote_peer_name?: string;
  remote_tab_id?: string;
  remote_channels?: RemoteChannelAvailabilityMap;
  ownership?: RemoteOwnership;
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
  closed_structures?: ClosedStructureView[];
  /** Aggregate Idle/Working/Waiting/Error/Dead per PTY-backed tab, mirrored from the
   *  backend's live status tracker so the tab rail hydrates correctly on
   *  reload instead of staying blank until the next status event. */
  tab_statuses?: Record<string, TabStatus>;
}



export type Team = Omit<import("../../gen/Team").Team, "name"> & { name?: import("../../gen/Team").Team["name"] | null; remote_peer_name?: string | null };

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
  cwd_missing?: boolean;
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

export type {
  Notification,
  NotificationAction,
  NotificationAnswer,
  NotificationPromptIdentity,
  CallerNotificationAction,
} from "./notificationTypes";



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

export type ExecutionEnd =
  | { kind: "reported" }
  | { kind: "process_ended"; detail: string }
  | { kind: "timed_out"; detail: string }
  | { kind: "cancelled" }
  | { kind: "orphaned" };

export type DiaryFinalization =
  | { state: "pending" }
  | { state: "succeeded"; diary_id: string }
  | { state: "partial"; diary_id?: string | null; reason: string }
  | { state: "failed"; diary_id?: string | null; reason: string };

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
  last_observed_at?: number | null;
  extraction_retry?: {
    attempt: number;
    parked: boolean;
    retry_after: number | null;
    reason: string | null;
  } | null;
  log_path?: string | null;
  agent_provider?: string | null;
  model?: string | null;
  session_id?: string | null;
  cwd?: string | null;
  execution_end?: ExecutionEnd | null;
  diary_id?: string | null;
  finalization?: DiaryFinalization | null;
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

export type CronRunStatus = "Running" | "AwaitingOwnerPrompt" | "Success" | "Failed" | "Cancelled";

export type CronCancelReason = "user_cancel" | "superseded" | "restart_reconcile" | "timeout" | "owner_closed";

export interface CronRunInfo {
  cancel_reason?: CronCancelReason | null;
  run_id: string;
  started_at: number;
  finished_at: number | null;
  status: CronRunStatus;
  tab_id: string | null;
  session_id?: string | null;
  cwd?: string | null;
  summary?: string | null;
}

export interface CronJob {
  protected?: boolean;
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
  /** Reasoning-effort id advertised by the agent provider. `null` = provider default. */
  reasoning_effort: string | null;
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





export interface ShellProfile {
  id: string;
  name: string;
  executable: string;
  args: string[];
  wsl_distro: string | null;
}































export interface NetworkIdentity { kind: string; network_id: string }
export interface NetworkTransportStatus {
  display_name: string;
  state: "not_installed" | "not_running" | "permission_needed" | "signed_out" | "starting" | "connected" | "unknown";
  identity: NetworkIdentity | null;
  network_name: string | null;
  user?: string | null;
  addresses: string[];
  peer_listener: boolean;
  encrypted: boolean;
  setup_actions: Array<{ kind: string; url: string }>;
}

export interface MeshStatus {
  enabled: boolean;
  instance_id: string;
  instance_name: string;
  tailscale_ip: string | null;
  local_ip: string | null;
  api_port: number;
  paired_count: number;
  transport: NetworkTransportStatus | null;
  web_ui: WebUiAnnouncement | null;
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

export const RELAY_PROTOCOL_VERSION = 3;

export type WebUiEndpoint =
  | { transport: "tailscale_https"; host: string; port: number }
  | { transport: "tailscale_http"; ip: string; port: number }
  | { transport: "lan_http"; ip: string; port: number };

export interface WebUiAnnouncement {
  protocol_version: number;
  endpoints: WebUiEndpoint[];
}

export interface MeshPeersResponse {
  paired: PairedPeer[];
  local_instance_name: string;
}

/// Per-provider settings stored under `agents.<id>` in the TOML config.
/// Mirrors `src-tauri/src/config/schema.rs::ProviderConfig`.


















// Provider descriptor wire types mirror `src-tauri/src/agent/provider.rs`.
// Frontend consumes them via `list_agent_providers`.

export interface ModelInfo {
  id: string;
  primary?: boolean | null;
  display_name: string;
  description?: string;
  available?: boolean | null;
  // A probe that could not run reports "unknown" here instead of collapsing
  // into `available: false`. See `lib/modelAvailability.ts`.
  availability?: "available" | "unavailable" | "unknown" | null;
  /** Catalog marks this model to lead the picker list, ahead of the rest. */
  featured?: boolean | null;
  /** Short chip rendered beside the model label in spawn pickers. */
  badge?: string | null;
  /** Provider-declared picker section; models sharing it render together. */
  group?: string | null;
  /** Omitted uses provider reasoning_efforts; [] means no selector. */
  reasoning_efforts?: ReasoningEffortInfo[] | null;
  /** Provider-reported default for this model, before provider-wide fallback. */
  default_reasoning_effort?: string;
}

/** Provider-owned grouping of its models, e.g. the upstream company a routed
 *  model belongs to. `id` matches the model-id prefix; `label` is the text the
 *  picker renders. Core never maps a vendor id to a label. */
export interface ModelGroup {
  id: string;
  label: string;
}

/** Cumulative usage the provider attributes to one model group. `cost_micros`
 *  is USD × 1_000_000 so the wire stays integer. The optional `spend_*` fields
 *  are the group's spend meter; the limit/remaining pair is present only when
 *  the provider can read a live quota — absent, never fabricated. */
export interface ModelGroupUsage {
  group: string;
  messages: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  cost_micros: number;
  spend_cents?: number;
  spend_limit_cents?: number;
  spend_remaining_cents?: number;
  spend_resets_at_ms?: number;
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
export type SystemPromptDelivery = { cli_flag: string } | "session_config" | "external";

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
  | {
      kind: "section";
      title: string;
      description?: string;
      fields: SettingField[];
    }
  | {
      kind: "toggle";
      key: string;
      label: string;
      default: boolean;
      description?: string;
    }
  | {
      kind: "text";
      key: string;
      label: string;
      default?: string;
      placeholder?: string;
      description?: string;
    }
  | {
      kind: "number";
      key: string;
      label: string;
      default?: number;
      min?: number;
      max?: number;
      description?: string;
    }
  | {
      kind: "select";
      key: string;
      label: string;
      options: [string, string][];
      default?: string;
      description?: string;
    }
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
          | {
              kind: "text";
              key: string;
              label: string;
              default?: string;
              placeholder?: string;
              description?: string;
            }
          | {
              kind: "textarea";
              key: string;
              label: string;
              default?: string;
              placeholder?: string;
              description?: string;
            }
          | {
              kind: "number";
              key: string;
              label: string;
              default?: number;
              min?: number;
              max?: number;
              description?: string;
            }
          | {
              kind: "json";
              key: string;
              label: string;
              default?: Record<string, unknown>;
              description?: string;
            }
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

export type ProviderAvailability = "available" | "unavailable" | "unknown";

export interface ModelCatalogueCapability {
  authority: "authoritative" | "free_form";
  liveDiscoveryRequired: boolean;
}

export type ModelSwitchMode = "live" | "live_command" | "next_turn" | "restart_resume" | "unsupported";

export interface ProviderAttachmentCapabilities {
  filePaths: boolean;
  inlineText: boolean;
  imagePaths: boolean;
}

export interface ProviderDescriptor {
  availability_checking?: boolean;
  general_agent_offer_when_absent?: boolean;
  /** Whether this provider can be a Neighbor's concierge here, for which machine access modes, and which of those this
   *  platform actually confines; the per-friend folder enforcement note stays the precise projection. */
  neighbor_concierge?: { supported: boolean; machine_access: readonly string[]; enforced_machine_access: readonly string[] };
  attachments?: ProviderAttachmentCapabilities;
  remote_task?: import("../../packages/plugin-sdk/src/remoteTasks").RemoteTaskCapability;
  agent_runtime?: {
    launchProfiles?: Readonly<Record<string, import("../../packages/plugin-sdk/src/agentRuntime").RuntimeLaunchProfile>>;
    runtimeModes?: readonly { id: string; displayName: string }[];
    hostApprovals?: boolean;
    serviceTiers?: readonly import("../../packages/plugin-sdk/src/agentRuntime").RuntimeServiceTier[];
    usage?: boolean;
    backgroundProcesses?: boolean;
    goals?: { actions: readonly string[] };
    threadHistory?: { actions: readonly ("fork" | "revert")[] };
  };
  supports_session_fork?: boolean;
  supports_subagent_visibility?: boolean;
  cli_status?: import("./providerCli").ProviderCliStatus;
  schema_version: number;
  id: string;
  catalogue_version?: number;
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
  model_catalogue?: ModelCatalogueCapability;
  /** Provider-owned groupings of `available_models`; empty ⇒ one flat list. */
  model_groups?: ModelGroup[];
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
  /** Provider writes custom model endpoints into its own config. */
  supports_model_endpoints?: boolean;
  supports_permission_bypass?: boolean;
  /// Provider can render a tab as chat (JSONL sessions or structured/DB-backed
  /// chat). Gates the chat/terminal toggle — replaces the old hardcoded set.
  supports_chat_ui?: boolean;
  /// Provider exposes a structured prompt source (claude AskUserQuestion in the
  /// JSONL transcript, opencode SSE bus) so pending prompts come from a handle
  /// rather than PTY scraping. Informational on the wire today.
  supports_structured_prompts?: boolean;
  supports_prompt_answers?: boolean;
  /// Provider recognizes its own permission prompts, so per-tab auto-confirm is offered.
  supports_permission_auto_confirm?: boolean;
  /// Binary name to probe via `which` when checking installation. Optional —
  /// providers without a CLI dependency leave this null.
  cli_binary?: string | null;
  /// Shell command that installs the CLI for the current platform. Used by
  /// the Install card on the per-provider Settings screen.
  cli_login?: { methods: { id: string; label: string; args: string[] }[]; urlPattern: string; timeoutMs: number } | null;
  install_command?: string | null;
  /// Short provider-declared note shown beside the name in pickers, e.g. a
  /// free-tier offer. Absent means the provider claims nothing.
  badge?: string | null;
  /** Provider-declared shell command that updates the CLI on this platform. */
  update_command?: string | null;
  /// How a live tab switches model: `restart_resume` relaunches the tab on its
  /// own session. Missing or `unsupported` means the model is chosen at spawn.
  model_switch?: ModelSwitchMode;
  /// Provider-declared in-agent context-compaction command. Consumed
  /// capability-keyed by the orchestrator refresh cycle. Missing = no compact.
  compact_command?: string | null;
  /// Optional `integration.trust_grant` capability: core invokes the
  /// provider-owned hook-trust grant once per integration install, scoped to
  /// these managed key(s). Informational on the wire.
  integration_trust_grant?: { managedKeys: string[] } | null;
  /// What enumeration observed about the launch binary on this machine.
  /// Absent (older peer) means nothing was observed — read it as `unknown`,
  /// never as the negative. Use `providerAvailability()` rather than this field.
  availability?: ProviderAvailability;
}

export interface ModelCatalogueChanged {
  provider: string;
  version: number;
}

export type { OrchestrationPairInput, OrchestrationPairResult } from "./orchestrationPairTypes";
