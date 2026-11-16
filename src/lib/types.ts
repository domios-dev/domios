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

export type ClosedTabView = Pick<import("../../gen/ClosedTabView").ClosedTabView, "tab_id" | "title" | "closed_at" | "index" | "orphaned" | "group_id" | "group_kind"> & Partial<import("../../gen/ClosedTabView").ClosedTabView>;

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
  /** The peer's own PTY for a remote tab; `pty_id` stays the composite id this machine routes by. */
  remote_pty_id?: string | null;
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
export type RemoteWorker = import("../../gen/RemoteWorker").RemoteWorker;

/** Map of local orchestrator tab id → its remote workers. */
export type RemoteWorkersByOrchestrator = Record<string, RemoteWorker[]>;

// Graph worker (Phase E memory visualization)

export type GraphWorkerStatus = import("../../gen/WorkerStatus").WorkerStatus | "failed" | "cancelled" | "hard_timeout" | "timeout" | "timed_out";

export type ExecutionEnd = import("../../gen/ExecutionEnd").ExecutionEnd;

export type DiaryFinalization = import("../../gen/DiaryFinalization").DiaryFinalization;

type WireGraphWorkerInfo = import("../../gen/GraphWorkerInfo").GraphWorkerInfo;
type LocalGraphWorkerInfo = { [K in keyof WireGraphWorkerInfo]: K extends "tab_id" | "progress_text" | "report" | "done_at" | "last_observed_at" | "extraction_retry" | "log_path" | "agent_provider" | "model" | "session_id" | "cwd" | "execution_end" | "diary_id" | "finalization" ? WireGraphWorkerInfo[K] | null : WireGraphWorkerInfo[K] };
export type GraphWorkerInfo = Required<Pick<LocalGraphWorkerInfo, "worker_id" | "intent" | "spawned_by" | "spawned_at" | "progress_text" | "touched_nodes" | "color" | "report" | "done_at"> > & Partial<Omit<LocalGraphWorkerInfo, "tab_id" | "status" | "finalization" | "extraction_retry">> & { tab_id: WireGraphWorkerInfo["tab_id"] | null; status: GraphWorkerStatus; finalization?: DiaryFinalization | null; extraction_retry?: Omit<import("../../gen/ExtractionRetry").ExtractionRetry, "reason"> & Partial<Pick<import("../../gen/ExtractionRetry").ExtractionRetry, "reason">> | null };

export type PluginActivity = Omit<import("../../gen/PluginActivity").PluginActivity, "percent" | "error" | "status"> & Partial<Pick<import("../../gen/PluginActivity").PluginActivity, "percent" | "error">> & { status: "running" | "done" | "error" };

export type SpawnWorkerReq = Omit<import("../../gen/WorkerSpawnBody").WorkerSpawnBody, "agent" | "parent"> & {agent?: NonNullable<import("../../gen/WorkerSpawnBody").WorkerSpawnBody["agent"]>; parent?: NonNullable<import("../../gen/WorkerSpawnBody").WorkerSpawnBody["parent"]>};

// Cron types

export type CronRunStatus = import("../../gen/CronRunStatus").CronRunStatus;

export type CronCancelReason = import("../../gen/CronCancelReason").CronCancelReason;

type WireCronRunInfo = import("../../gen/CronRunInfo").CronRunInfo;
type LocalCronRunInfo = { [K in keyof WireCronRunInfo]: K extends "cancel_reason" | "finished_at" | "tab_id" | "session_id" | "cwd" | "summary" ? WireCronRunInfo[K] | null : WireCronRunInfo[K] };
export type CronRunInfo = Required<Pick<LocalCronRunInfo, "run_id" | "started_at" | "finished_at" | "status" | "tab_id"> > & Partial<Omit<LocalCronRunInfo, never>>;

type WireCronJob = import("../../gen/CronJob").CronJob;
type LocalCronJob = { [K in keyof WireCronJob]: K extends "description" | "team_id" | "model" | "reasoning_effort" | "cwd" | "role" | "creator_tab_id" | "report_target_tab_id" | "last_run" | "next_run" | "jitter_max_ms" ? WireCronJob[K] | null : WireCronJob[K] };
export type CronJob = Required<Pick<LocalCronJob, "id" | "name" | "description" | "workspace_id" | "schedule" | "enabled" | "agent" | "model" | "reasoning_effort" | "task" | "cwd" | "role" | "oneshot" | "max_history" | "created_at" | "next_run" | "jitter_max_ms"> > & Partial<Omit<LocalCronJob, "last_run">> & { last_run: CronRunInfo | null };

export type ShellProfile = import("../../gen/ShellProfile").ShellProfile;































export type NetworkIdentity = import("../../gen/NetworkIdentity").NetworkIdentity;
export type NetworkTransportStatus = import("../../gen/TransportStatus").TransportStatus;

export type MeshStatus = Omit<import("../../gen/MeshStatus").MeshStatus, "build"> & Partial<Pick<import("../../gen/MeshStatus").MeshStatus, "build">>;

export type PeerAddress = import("../../gen/PeerAddress").PeerAddress;

export type PairedPeer = Pick<import("../../gen/PairedPeer").PairedPeer, "instance_id" | "instance_name" | "addresses" | "api_port" | "public_key" | "paired_at" | "last_seen" | "online" | "is_primary" | "access_level" | "version" | "relay_protocol_version"> & Partial<import("../../gen/PairedPeer").PairedPeer>;

export const RELAY_PROTOCOL_VERSION = 3;

export type WebUiEndpoint = import("../../gen/WebUiEndpoint").WebUiEndpoint;

export type WebUiAnnouncement = import("../../gen/WebUiAnnouncement").WebUiAnnouncement;

export type MeshPeersResponse = Pick<import("../../gen/MeshPeersResponse").MeshPeersResponse, "paired" | "local_instance_name"> & Partial<import("../../gen/MeshPeersResponse").MeshPeersResponse>;

/// Per-provider settings stored under `agents.<id>` in the TOML config.
/// Mirrors `src-tauri/src/config/schema.rs::ProviderConfig`.


















// Provider descriptor wire types mirror `src-tauri/src/agent/provider.rs`.
// Frontend consumes them via `list_agent_providers`.

type WireModelInfo = import("../../gen/ModelInfo").ModelInfo;
export type ModelInfo = { [K in keyof WireModelInfo]: K extends "primary" | "available" | "availability" | "featured" | "badge" | "group" | "reasoning_efforts" ? WireModelInfo[K] | null : WireModelInfo[K] };

/** Provider-owned grouping of its models, e.g. the upstream company a routed
 *  model belongs to. `id` matches the model-id prefix; `label` is the text the
 *  picker renders. Core never maps a vendor id to a label. */
export type ModelGroup = import("../../gen/ModelGroup").ModelGroup;

/** Cumulative usage the provider attributes to one model group. `cost_micros`
 *  is USD × 1_000_000 so the wire stays integer. The optional `spend_*` fields
 *  are the group's spend meter; the limit/remaining pair is present only when
 *  the provider can read a live quota — absent, never fabricated. */
export type ModelGroupUsage = import("../../gen/ModelGroupUsage").ModelGroupUsage;

export type ReasoningEffortInfo = import("../../gen/ReasoningEffortInfo").ReasoningEffortInfo;

export type PresetInfo = import("../../gen/PresetInfo").PresetInfo;

/** Runtime backing a provider — the spawn discriminant (was the `kind` string). */
export type ProviderTransport = import("../../gen/ProviderTransport").ProviderTransport;

/** Behavior surface a provider exposes for configuration. Tagged on `model`. */
export type BehaviorConfig = import("../../gen/BehaviorConfig").BehaviorConfig;

/** How a system prompt reaches the provider. */
export type SystemPromptDelivery = import("../../gen/SystemPromptDelivery").SystemPromptDelivery;

export type NoteLevel = import("../../gen/NoteLevel").NoteLevel;
export type BadgeStyle = import("../../gen/BadgeStyle").BadgeStyle;
export type ActionStyle = import("../../gen/ActionStyle").ActionStyle;
export type PathOpenKind = import("../../gen/PathOpenKind").PathOpenKind;

export type BadgeVariant = import("../../gen/BadgeVariant").BadgeVariant;

/// Discriminated union — `kind` field tags the variant. Mirror of
/// `SettingField` enum (snake_case rename) on the Rust side.
export type SettingField = import("../../gen/SettingField").SettingField;

export type ProviderAvailability = import("../../gen/ProviderAvailability").ProviderAvailability;

export type ModelCatalogueCapability = Pick<import("../../gen/ModelCatalogueCapability").ModelCatalogueCapability, "authority" | "liveDiscoveryRequired"> & Partial<import("../../gen/ModelCatalogueCapability").ModelCatalogueCapability>;

export type ModelSwitchMode = import("../../gen/ModelSwitchMode").ModelSwitchMode;

export type ProviderAttachmentCapabilities = import("../../gen/AttachmentCapabilities").AttachmentCapabilities;

type WireProviderDescriptor = import("../../gen/ProviderDescriptor").ProviderDescriptor;
type ProviderRuntimeView = Partial<Omit<import("../../gen/AgentRuntimeCapability").AgentRuntimeCapability, "launchProfiles" | "runtimeModes" | "serviceTiers" | "goals" | "threadHistory">> & { launchProfiles?: Readonly<Record<string, import("../../packages/plugin-sdk/src/agentRuntime").RuntimeLaunchProfile>>; runtimeModes?: readonly import("../../gen/RuntimeModeDescriptor").RuntimeModeDescriptor[]; serviceTiers?: readonly import("../../gen/RuntimeServiceTier").RuntimeServiceTier[]; goals?: { actions: readonly string[] }; threadHistory?: { actions: readonly import("../../gen/RuntimeThreadHistoryAction").RuntimeThreadHistoryAction[] } };
export type ProviderDescriptor = Pick<WireProviderDescriptor, "schema_version" | "id" | "display_name" | "transport" | "system_prompt_delivery" | "settings_schema" | "notifications_schema" | "supports_lazy_skills" | "has_hooks"> & Partial<Omit<WireProviderDescriptor, "agent_runtime" | "cli_status" | "available_models" | "model_catalogue" | "cli_login" | "neighbor_concierge" | "cli_binary" | "install_command" | "badge" | "update_command" | "compact_command" | "integration_trust_grant"> > & { availability_checking?: boolean; available_models: ModelInfo[]; model_catalogue?: ModelCatalogueCapability; agent_runtime?: ProviderRuntimeView; cli_status?: import("./providerCli").ProviderCliStatus; neighbor_concierge?: Omit<import("../../gen/NeighborConciergeCapability").NeighborConciergeCapability, "machine_access" | "enforced_machine_access"> & { machine_access: readonly string[]; enforced_machine_access: readonly string[] }; cli_login?: Omit<import("../../gen/CliLoginCapability").CliLoginCapability, "codeInput"> & Partial<Pick<import("../../gen/CliLoginCapability").CliLoginCapability, "codeInput">> | null; cli_binary?: WireProviderDescriptor["cli_binary"] | null; install_command?: WireProviderDescriptor["install_command"] | null; badge?: WireProviderDescriptor["badge"] | null; update_command?: WireProviderDescriptor["update_command"] | null; compact_command?: WireProviderDescriptor["compact_command"] | null; integration_trust_grant?: WireProviderDescriptor["integration_trust_grant"] | null };

export type ModelCatalogueChanged = import("../../gen/ModelCatalogueChanged").ModelCatalogueChanged;

export type { OrchestrationPairInput, OrchestrationPairResult } from "./orchestrationPairTypes";
