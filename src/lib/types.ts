type NullableOptionals<T> = { [K in keyof T]: undefined extends T[K] ? T[K] | null : T[K] };

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

export type TitleLevel = import("../../gen/TitleLevel").TitleLevel;
export const TitleLevel = { Default: "Default", Auto: "Auto", Agent: "Agent", User: "User" } as const satisfies Record<TitleLevel, TitleLevel>;

export type Workspace = Pick<import("../../gen/Workspace").Workspace, "id" | "name" | "tabs" | "active_tab_id" | "cwd"> & Partial<import("../../gen/Workspace").Workspace>;

export type ClosedTab = Pick<import("../../gen/ClosedTab").ClosedTab, "tab_id" | "title" | "closed_at"> & Partial<import("../../gen/ClosedTab").ClosedTab>;

export type ClosedTabView = Pick<import("./multiplexerWireFallbacks").ClosedTabView, "tab_id" | "title" | "closed_at" | "index" | "orphaned" | "group_id" | "group_kind"> & Partial<import("./multiplexerWireFallbacks").ClosedTabView>;

export type { ClosedStructureView } from "../../gen/ClosedStructureView";

export type SurfaceTabBacking = Extract<import("../../gen/TabBacking").TabBacking, { kind: "surface" }>;

export type TabBacking = import("../../gen/TabBacking").TabBacking;

export type WatcherPolicy = import("../../gen/WatcherPolicy").WatcherPolicy;

export type WatcherSpec = Pick<import("../../gen/WatcherSpec").WatcherSpec, "charter" | "policy"> & Partial<Omit<import("../../gen/WatcherSpec").WatcherSpec, "observations" | "state">> & { observations?: unknown; state?: unknown };

export type TabAgentState = Omit<NullableOptionals<import("../../gen/AgentLiveState").AgentLiveState>, "subtype"> & Pick<import("../../gen/AgentLiveState").AgentLiveState, "subtype">;

export type TabBootStageEvent = Pick<import("../../gen/TabBootStageEvent").TabBootStageEvent, "tab_id" | "stage"> & Partial<import("../../gen/TabBootStageEvent").TabBootStageEvent>;

type TabClearableFields = "agent_runtime_config" | "agent_runtime_state" | "pending_model_selection" | "pending_model_switch_observation" | "close_restore" | "shell_profile" | "icon" | "workspace_id" | "pty_id" | "description" | "cwd" | "agent_type" | "agent_session_id" | "session_binding_nonce" | "reported_session_binding_nonce" | "agent_session_name" | "agent_session_model" | "agent_requested_model" | "structured_runtime_mode" | "agent_provider_resolved_model" | "agent_label" | "role" | "role_profile" | "team_id" | "spawned_by_tab_id" | "managed" | "parent_tab_id" | "pending_prompt" | "pending_prompt_kind" | "agent_state" | "worker_id" | "slept_at_ms" | "last_active_at_ms" | "boot_stage" | "lifecycle_epoch" | "lifecycle_operation_id" | "boot_failure_reason" | "provisional_launch" | "watcher";
type LocalTabNullability<T> = { [K in keyof T]: K extends TabClearableFields ? T[K] | null : T[K] };
type WireSnapshotTab = import("../../gen/MultiplexerSnapshot").MultiplexerSnapshot["tabs"][string];
export type Tab = Pick<WireSnapshotTab, "id" | "pty_id" | "tab_type" | "title" | "cwd"> & Partial<Omit<LocalTabNullability<WireSnapshotTab>, "agent_runtime_config" | "agent_runtime_questions" | "agent_runtime_state" | "title_pending_local" | "remote_peer_id" | "remote_peer_name" | "remote_tab_id" | "remote_channels" | "provider_signals" | "pending_model_selection" | "pending_model_switch_observation" | "context_epoch_archives" | "watcher" | "agent_state">> & {
  workspace_id?: import("../../gen/Workspace").Workspace["id"] | null;
  session_bound?: import("../../gen/TabBootStageEvent").TabBootStageEvent["session_bound"];
  watcher?: WatcherSpec | null;
  agent_state?: TabAgentState | null;
  agent_runtime_config?: { confirmedServiceTier?: { selection: ServiceTierSelection; sessionId: string; bindingNonce: string | null }; serviceTier?: ServiceTierSelection; serviceTierAvailability?: Record<string, readonly import("../../packages/plugin-sdk/src/agentRuntime").RuntimeServiceTier[]>; serviceTierAvailabilityBinding?: {sessionId:string;bindingNonce:string|null}; systemPrompt?: never; [key: string]: unknown } | null;
  agent_runtime_questions?: import("./agentRuntimeQuestions").AgentRuntimeQuestion[];
  agent_runtime_state?: import("./agentRuntimeState").AgentRuntimeState | null;
  provider_signals?: ProviderSignals;
  title_pending_local?: boolean;
  remote_peer_id?: string;
  remote_peer_name?: string;
  remote_tab_id?: string;
  remote_channels?: RemoteChannelAvailabilityMap;
  pending_model_selection?: Omit<import("../../gen/PendingModelSelection").PendingModelSelection, "confirmation_pending"> & Partial<Pick<import("../../gen/PendingModelSelection").PendingModelSelection, "confirmation_pending">> | null;
  pending_model_switch_observation?: Pick<import("../../gen/PendingModelSwitchObservation").PendingModelSwitchObservation, "launched_model"> & Partial<import("../../gen/PendingModelSwitchObservation").PendingModelSwitchObservation> | null;
  context_epoch_archives?: Array<Pick<import("../../gen/ContextEpochArchive").ContextEpochArchive, "provider" | "cwd" | "session_id"> & Partial<import("../../gen/ContextEpochArchive").ContextEpochArchive>>;
};

export type MultiplexerState = Omit<import("../../gen/MultiplexerSnapshot").MultiplexerSnapshot, "workspaces" | "tabs" | "teams" | "recently_closed" | "generation"> & { workspaces: Workspace[]; tabs: Record<string, Tab>; teams?: Team[]; recently_closed?: ClosedTabView[]; generation?: number };



export type Team = Omit<import("../../gen/Team").Team, "name"> & { name?: import("../../gen/Team").Team["name"] | null; remote_peer_name?: string | null };

export type AtomicSpawn = import("../../gen/AtomicSpawn").AtomicSpawn<MultiplexerState>;

export type WorkspaceInfo = import("../../gen/WorkspaceInfo").WorkspaceInfo;

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

export type ShellProfile = import("../../gen/ShellProfile").ShellProfile;































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
