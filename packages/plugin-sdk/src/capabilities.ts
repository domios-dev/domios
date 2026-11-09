import type {OptionalWireCases, OptionalWireFields} from "./wireViews";
import type { AgentRuntime } from "./agentRuntime";
import type { ParsedSessionRecord,SessionRecordsMeta } from "./jsonlRecords";
export type PreTrustOutcome = import("./gen/PreTrustOutcome").PreTrustOutcome;

// Plugin-facing capability contracts. Each capability is one Rust trait the host
// adapts to a `plugin.js` export; shapes mirror how the host consumes them.

// ── Shared context ──

import type { ChatMsg } from "./gen/ChatMsg";
import type { ModelProviderConfig } from "./modelProviders";
import type { ProviderAuthStatus,ProviderSessionObservations } from "./providerSignals";

/** Manifest spawn.turnEvidenceNormalization; applies symmetrically to exact user-turn matching. */
export type TurnEvidenceNormalization = OptionalWireFields<import("./gen/TurnEvidenceNormalization").TurnEvidenceNormalization, "unwrapPastedContentWrapper" | "collapseHorizontalWhitespace" | "dropSupplementaryCharacters" | "dropLineBreaks">;

/** Manifest spawn contract; absent means the provider does not restore chat on resume. */
export interface ResumeHistoryCapability {
  restoresConversationHistoryOnResume?: boolean;
}

export type SessionResumeCapability = import("./gen/SessionResume").SessionResume;

export type {TabContext} from "./tabContext";
import type {TabContext} from "./tabContext";

export type SessionDeltaContext = import("./gen/SessionDeltaContext").SessionDeltaContext;

export type SessionModelTranscriptBoundary = import("./gen/SessionModelTranscriptBoundary").SessionModelTranscriptBoundary;

// ── statusBubble ──

export type BubbleTone = import("./gen/Tone").Tone;

export type StatusBubble = OptionalWireFields<import("./gen/BubbleData").BubbleData, "icon" | "tooltip" | "action">;

// ── glanceView (mirrors src-tauri/crates/codeterm-service-core/src/view/mod.rs ViewNode) ──

export type Tone = import("./gen/Tone").Tone;
export type Spacing = import("./gen/Spacing").Spacing;
export type NoteLevel = import("./gen/ViewNoteLevel").ViewNoteLevel;

export type NodeStyle = OptionalWireFields<import("./gen/NodeStyle").NodeStyle, "tone" | "spacing">;

export type ViewNode = OptionalWireCases<Exclude<import("./gen/ViewNode").ViewNode, {kind: "row" | "stack"}>, "tone" | "level"> | ViewNodeRow | ViewNodeStack;

export type GlanceView = Omit<OptionalWireFields<import("./gen/GlanceView").GlanceView, "title">, "nodes"> & {nodes: ViewNode[]};

// ── view (sandboxed iframe UI bridge target) ──

export type ViewCall = (method: string, args: unknown, context: ViewCallContext) => unknown;

// ── agentProvider ──
// Method set mirrors plugins/provider-*/plugin.js + the AgentProvider trait.

export type ContextAttach = import("./gen/ContextAttach").ContextAttach;
export type ContextFiles = import("./gen/TaskFiles").TaskFiles;
export type HostOwnedCapabilities = import("./gen/HostOwnedCapabilities").HostOwnedCapabilities;

export type LaunchParams = import("./gen/LaunchParams").LaunchParams;

export type LaunchArtifactSweepRequest = OptionalWireFields<import("./gen/LaunchArtifactSweepRequest").LaunchArtifactSweepRequest, "goneInstanceIds">;

export type LaunchArtifactSweepReport = import("./gen/LaunchArtifactSweepReport").LaunchArtifactSweepReport;

/** One process in the tab's own PTY tree; `startToken` is its OS process start time. */
export type LaunchProcess = import("./gen/LaunchProcess").LaunchProcess;

/** Everything the host knows about one launch, and nothing about any other. */
export type LaunchSessionEvidence = OptionalWireFields<import("./gen/LaunchSessionEvidence").LaunchSessionEvidence, "launchMarker">;

/** Provider-authoritative top-level completion, including automatic continuations. */
export type ReplyRelayEvidence = import("./gen/ReplyRelayEvidence").ReplyRelayEvidence;

export type ReplyRelayTurn = import("./gen/ReplyRelayTurn").ReplyRelayTurn;

export type LaunchSessionDetection = import("./gen/LaunchSessionDetection").LaunchSessionDetection;

/** Full launch context for a resumed session, not just its id. */
export type ResumeParams = OptionalWireFields<import("./gen/AgentResumeParams").AgentResumeParams, "cwd" | "contextFiles" | "toolLessInstructionsPath" | "skipPermissions" | "hostOwned" | "agentName" | "instanceId" | "systemPrompt" | "args">;

// picker can't read its layout (e.g. opencode's single-row options).
export type ParsedPrompt = import("./gen/ParsedPrompt").ParsedPrompt;

// ── tab-state classification (mirrors codeterm-plugin tab_state.rs) ──

export type TabState = import("./gen/TabState").TabState;

/** Host-built snapshot of the tab's OWN session/screen handed to classifyTabState. */
export type TabStateSnapshot = OptionalWireFields<import("./gen/TabStateSnapshot").TabStateSnapshot, "sessionId" | "cwd">;

export type DelayedResume = OptionalWireFields<import("./gen/DelayedResume").DelayedResume, "thenKeys">;

/** The action a classifier recommends; the host reactor branches only on these generic fields. */
export type TabStateAction = Omit<import("./gen/TabStateAction").TabStateAction, "surface"> & {surface?: Omit<NonNullable<import("./gen/TabStateAction").TabStateAction["surface"]>, "payload"> & {payload: unknown}};

export type TabStateClassification = Omit<import("./gen/TabStateClassification").TabStateClassification, "action"> & {action?: TabStateAction};

// ── declarative settings schema (mirrors codeterm-plugin provider/settings.rs) ──

/** One field of the declarative settings UI, tagged by `kind` (section, toggle, text, …). */
export type SettingField = import("./gen/SettingField").SettingField;

export interface AgentProvider {
  sessionResume?(): SessionResumeCapability;
  authStatusProbe?(): import("./gen/AuthStatusProbe").AuthStatusProbe | null;
  tracksBackgroundWork?(): boolean;
  parseBackgroundWork?(record: unknown): import("./gen/BackgroundWorkObservation").BackgroundWorkObservation[];
  reasoningEffortLaunchArgsForModel?(model: string | null, effort: string): string[];
  modelCatalogueSeed?(): ModelInfo[];
  parseAuthStatus?(record: unknown): ProviderAuthStatus | null;
  parseSessionObservations?(record: unknown): ProviderSessionObservations | null;
  parseContextUsage?(record: unknown): import("./gen/TurnContext").TurnContext | null;
  permissionHookCapability?(): import("./gen/PermissionHookCapability").PermissionHookCapability | null;
  starterPromptText?(prompt: "no_starter" | "assigned_task" | "idle_wakeup" | "team_bootstrap" | "startup_handshake"): string | undefined;
  detectFromTitle?(title: string): boolean;
  detectFromOutput?(text: string): boolean;
  screenHasTui?(screen: string): boolean;
  /** Allow bounded submit-key recovery without an authenticated provider session. */
  supportsUnboundSubmitRecovery?(): boolean;
  isOutputNoise?(text: string): boolean;
  hasEventMarkers?(chunk: string): boolean;
  detectEvents?(text: string): string[];
  extractSessionName?(screen: string): string | null;
  buildLaunchCommand?(params: LaunchParams): string;
  buildLaunchSpec?(params: LaunchParams | { resume: ResumeParams }): (OptionalWireFields<import("./gen/LaunchSpec").LaunchSpec, "envSet" | "envUnset">) | null;
  releaseTabLaunchArtifacts?(tabId: string, instanceId: string): boolean;
  /** Persist provider-owned folder trust after this tab's trust dialog closes. */
  folderTrustAnswered?(tabId: string, instanceId: string): boolean;
  sweepLaunchArtifacts?(request: LaunchArtifactSweepRequest): LaunchArtifactSweepReport | null;
  buildResumeCommand?(sessionId: string, skipPermissions?: boolean): string | null;
  /** Implement only when a resumed session drops the launch-time system prompt. */
  buildResumeCommandWithContext?(params: ResumeParams): string | null;
  buildPromptResponse?(optionIndex: number, screen?: string): string[];
  /** Directory-trust dialog on the live screen: one base64 key, `[]` to hold it, `null` when none is shown. */
  directoryTrustPromptResponse?(screen: string): string[] | null;
  /** Best-effort persistence for an exact folder already authorized by the host. */
  preTrustDirectory?(path: string): PreTrustOutcome;
  /** Whether the provider's own config already trusts this exact folder. */
  isDirectoryTrusted?(path: string): boolean;
  /** Effective launch-folder trust, including the provider's trust-root precedence. */
  directoryTrustState?(path: string, skipPermissions: boolean): "trusted" | "untrusted" | "unknown";
  /** One next base64 key plus its stable choice/focus identity, or null. */
  launchOnboardingResponse?(screen: string): (import("./gen/LaunchOnboardingResponse").LaunchOnboardingResponse) | null;
  /** Exact option value for that same narrow, provider-declared onboarding response. */
  launchOnboardingSafeChoice?(screen: string): string | null;
  /** Parse only the active directory-trust dialog for tab status/respond. */
  parseDirectoryTrustPrompt?(screen: string): ParsedPrompt | null;
  /** Tool-permission approval for the hook-reported tool; `null` leaves the prompt to the user. */
  permissionPromptResponse?(
    toolName: string,
    prompt: import("./gen/PendingPrompt").PendingPrompt,
    screen?: string,
  ): (import("./gen/PermissionPromptWireResponse").PermissionPromptWireResponse) | null;
  /** Safe response role for a prompt only after core accepted its classifier. */
  classifiedPromptAutoResponse?(state: TabState, screen: string): "skip" | null;
  updatePromptSafeChoice?(prompt: ParsedPrompt): string | null;
  updatePromptAvailability?(screen: string): (import("./gen/ProviderUpdateAvailability").ProviderUpdateAvailability) | null;
  parsePrompt?(text: string): ParsedPrompt | null;
  parseSwitchConfirmationPrompt?(screen: string, selection: string): ParsedPrompt | null;
  sessionFilePath?(cwd: string, sessionId: string): string | null;
  sessionJsonlPath?(cwd: string, sessionId: string): string | null;
  /** Further usage logs owned by this exact session file (e.g. sub-agent transcripts beside it). */
  sessionUsageSources?(sessionFilePath: string, sessionId: string): string[];
  /** Every usage log this provider owns for the session; `sessionId` scopes a log shared by many sessions. */
  usageSources?(cwd: string, sessionId: string): (OptionalWireFields<import("./gen/WireUsageSource").WireUsageSource, "sessionId">)[];
  sessionExists?(cwd: string, sessionId: string): boolean | null;
  sessionCreatedMs?(cwd: string, sessionId: string): number | null;
  /** Each returned row carries `recordIndex`, the chunk line it was parsed from. */
  parseSessionDelta?(chunk: string, context?: SessionDeltaContext): unknown;
  /** Rows retain each source record's original recordIndex for host byte anchoring. */
  parseSessionRecords?(records: readonly ParsedSessionRecord[], meta: SessionRecordsMeta): unknown;
  parseSessionMessageRecords?(records: readonly ParsedSessionRecord[], meta: SessionRecordsMeta): unknown[];
  detectSessionId?(cwd: string, exclude?: string, maxAgeMs?: number): string | null;
  /** Session named by this launch's own provider record; null when none names it. */
  detectLaunchSession?(evidence: LaunchSessionEvidence): LaunchSessionDetection | null;
  /** Requires manifest capabilities.replyRelay. Read after a captured UTF-8 append boundary; null without authoritative completion. */
  readReplyRelayTurn?(cwd: string, sessionId: string, afterOffset: number): ReplyRelayEvidence | null;
  detectActiveSession?(cwd: string, currentSid?: string): string | null;
  sessionModelIdentityPath?(cwd: string, sessionId: string): string | null;
  detectSessionModel?(cwd: string, sessionId: string): string | null;
  sessionModelTranscriptBoundary?(cwd: string, sessionId: string): SessionModelTranscriptBoundary | null;
  detectSessionModelAfterBoundary?(
    cwd: string,
    sessionId: string,
    boundary: SessionModelTranscriptBoundary,
  ): string | null;
  detectSessionReasoningEffort?(cwd: string, sessionId: string): string | null;
  enumerateSessions?(): unknown[];
  /** The one record owning exactly this id, or null; never scans session history. */
  findSession?(sessionId: string): unknown | null;
  describeSession?(
    filePath: string,
    head?: string,
  ): import("./gen/SessionDescription").SessionDescription | null;
  parseUsage?(rawText: string, nowMs: number): unknown;
  fetchUsage?(nowMs: number): unknown;
  /** Host-observed CLI identity, computed before entering the plugin VM. */
  discoverModels?(binaryFingerprint?: string | null): ModelInfo[] | null;
  /** Cumulative usage per model group (matching manifest `modelGroups` ids, or a
   *  bare model-id prefix). The picker shows it for the selected model's group. */
  modelGroupUsage?(): ModelGroupUsage[];
  parseSessionMessages?(text: string): unknown[];
  /** One session's messages from a store shared by every session, newest-first within
   *  `query.maxBytes`; a message lacking any `hintTerms` entry may be omitted. Null = unreadable. */
  readSessionContent?(
    sessionId: string,
    query: (import("./gen/SessionContentQuery").SessionContentQuery),
  ): (Omit<import("./gen/WireSessionContent").WireSessionContent, "messages" | "truncated"> & {messages: Array<OptionalWireFields<import("./gen/SerdeSessionMessage").SerdeSessionMessage, "ts" | "isMeta">>; truncated?: import("./gen/WireSessionContent").WireSessionContent["truncated"]}) | null;
  structuredToolFileDiffs?(part: unknown): FileDiff[] | null;
  usesStructuredChat?(): boolean;
  readStructuredChat?(cwd: string, sessionId: string, cursor?: string | null): unknown;
  checkIntegration?(): unknown;
  installIntegration?(apiPort: number, options: (OptionalWireFields<import("./gen/IntegrationInstallOptions").IntegrationInstallOptions, "repair">)): unknown;
  uninstallIntegration?(): unknown;
  restoreIntegration?(): unknown;
  ensureIntegrationOnStartup?(apiPort: number): unknown;
  installBridgeRule?(payload: string): unknown;
  uninstallBridgeRule?(): unknown;
  /** Classify the tab's lifecycle state from its own screen snapshot. */
  classifyTabState?(snapshot: TabStateSnapshot): TabStateClassification | null;
  /** Extract interactive prompt elements from one rich chat message; requires manifest spawn.interactiveEvents. */
  parseInteractiveEvents?(msg: unknown): unknown[];
  /** Provider-specific settings sections; absent ⇒ manifest-derived sections. */
  settingsSchema?(): SettingField[];
  notificationsSchema?(): SettingField[];
}

// ── secretBackend (mirrors the bitwarden plugin + JsSecretBackend) ──

export type SecretCreds = import("./gen/UnlockCredentials").UnlockCredentials;

// core SecretStore trait: `{ ok }` / `{ ok: <payload> }` / `{ error: <kind> }`.
export type SecretError = import("./gen/SecretError").SecretError;
export type SecretEnvelope<T = unknown> = OptionalWireFields<import("./gen/SecretEnvelope").SecretEnvelope<T>, "ok" | "error">;

// secretStatus is the one method that returns a bare StoreStatus, not an envelope.
export type SecretStatus = import("./gen/StoreStatus").StoreStatus;

export type SecretSaveRequest = import("./gen/SaveRequest").SaveRequest;

export type SecretListFilter = OptionalWireFields<import("./gen/ListFilter").ListFilter, "organization" | "collection" | "folder" | "limit">;

export interface SecretBackend {
  secretStatus(): SecretStatus;
  secretUnlock?(creds: SecretCreds): unknown;
  secretLock?(): unknown;
  secretLogout?(): unknown;
  secretGet?(name: string): unknown;
  secretSet?(name: string, value: string): unknown;
  secretList?(filter?: SecretListFilter): unknown;
  secretDelete?(name: string): unknown;
  secretSave?(req: SecretSaveRequest): unknown;
  secretGetItem?(nameOrId: string): unknown;
  secretSearch?(query: string): unknown;
  secretOrganizations?(): unknown;
  secretCollections?(organizationId?: string): unknown;
  secretSync?(): unknown;
  secretInit?(): unknown;
}

// ── transcriber (mirrors the transcriber plugin + Transcriber trait) ──

export type TranscribeResult = OptionalWireFields<import("./gen/TranscribeOut").TranscribeOut, "text" | "error">;

export interface Transcriber {
  transcribe(path: string, lang?: string): TranscribeResult;
  transcribeStatus?(): unknown;
}

// ─── converter + graphNodes (declared conversions and event sources) ───

export type MediaKind = import("./gen/MediaKind").MediaKind;

export type GraphNodeConversion = import("./gen/Conversion").Conversion;

/** One declared conversion of an input file. */
export type ConvertRequest = import("./gen/ConvertRequest").ConvertRequest;

/** `{text}` | `{path}` (inside outputDir) | `{error}`. */
export type ConvertResult = OptionalWireFields<import("./gen/ConvertOut").ConvertOut, "text" | "path" | "error">;

/** Mirrors the core Converter trait; the host chains declared edges into plans. */
export interface Converter {
  convert(request: ConvertRequest): ConvertResult;
}

// NodeSchema subset: an object of scalar or one-level scalar-array properties.
// No $ref, combinators, patternProperties or nested objects.
export type NodeScalarType = import("./gen/NodeScalarType").NodeScalarType;
export type NodeScalarSchema = import("./gen/AuthoredNodeScalarSchema").AuthoredNodeScalarSchema;
export type NodeArraySchema = import("./gen/AuthoredNodeArraySchema").AuthoredNodeArraySchema;
export type NodePropertySchema = NodeScalarSchema | NodeArraySchema;
export type NodeSchema = OptionalWireFields<import("./gen/AuthoredNodeSchema").AuthoredNodeSchema, "required" | "properties">;
export type EventSourceDeclaration = import("./gen/EventSourceDeclaration").EventSourceDeclaration;
export type GraphNodesManifestCapability = OptionalWireFields<import("./gen/GraphNodesManifestCapability").GraphNodesManifestCapability, "steps" | "converters" | "eventSources">;

export type DecisionQuestion = OptionalWireCases<import("./gen/DecisionQuestion").DecisionQuestion, "criteria">;

export type DecisionRequest = Omit<import("./gen/DecisionRequest").DecisionRequest, "question"> & {question: DecisionQuestion};

export type NamedDecisionQuestion = Omit<import("./gen/NamedDecisionQuestion").NamedDecisionQuestion, "question"> & {question: DecisionQuestion};

export type DecisionQuestionsRequest = Omit<import("./gen/DecisionQuestionsRequest").DecisionQuestionsRequest, "questions"> & {questions: Record<string, NamedDecisionQuestion>};

export type DecisionAnswers = DecisionResponse["answers"];

export type DecisionUsage = import("./gen/DecisionUsage").DecisionUsage;

export type DecisionPricing = import("./gen/DecisionPricing").DecisionPricing;

export type DecisionResponse = import("./gen/DecisionResponse").DecisionResponse;

export type DecisionPrimitives = import("./gen/DecisionPrimitives").DecisionPrimitives;

export type DecisionModelInfo = import("./gen/DecisionModelInfo").DecisionModelInfo;

export type DecisionAnswer = import("./gen/DecisionAnswer").DecisionAnswer;

export interface DecisionModelCapability {
  /** Declares that choice confidence is calibrated for single-decision authorization. */
  supportsCalibratedChoiceConfidence?(): boolean;
  /** Declares that decideQuestions sends at most one physical request. */
  supportsQuestionBatch?(): boolean;
  modelId?(): string | null;
  selectModel?(id: string): boolean;
  metadata?(): (Partial<{[K in keyof import("./gen/DecisionMetadata").DecisionMetadata]: NonNullable<import("./gen/DecisionMetadata").DecisionMetadata[K]>}>);
  pricing?(): DecisionPricing | null;
  decide(request: DecisionRequest, remainingMs?: number): DecisionAnswer | DecisionFailure;
  /** Named questions over shared state; omitted methods use sequential decide calls. */
  decideQuestions?(request: DecisionQuestionsRequest, remainingMs?: number): DecisionResponse | DecisionFailure;
  models(binaryFingerprint?: string | null): DecisionModelInfo[];
  primitives(): DecisionPrimitives;
}

export type DecisionFailure = import("./gen/DecisionFailure").DecisionFailure;

/** Manifest token policy for `capabilities.decisionModel`. Bare true is optional. */
export type DecisionModelTokenMode = import("./gen/TokenMode").TokenMode;
export type DecisionModelManifestCapability = import("./gen/DecisionModelManifestCapability").DecisionModelManifestCapability ;

// ── chatBackend (non-PTY chat transport; mirrors the ChatBackendProvider trait) ──

export type ToolEventKind =
  | "command"
  | "edit"
  | "read"
  | "search"
  | "task"
  | "fetch"
  | "codeterm"
  | "web"
  | "image"
  | "agent"
  | "generic";

export interface ToolEdit {
  path: string;
  old: string;
  new: string;
}

export type ChatTimelineMarker = import("./gen/ChatTimelineMarker").ChatTimelineMarker;

export type ChatSessionIntent = OptionalWireFields<import("./gen/ChatSessionIntent").ChatSessionIntent, "title" | "goal" | "goalStatus">;

/** One hunk as the provider supplied it; every line keeps its ` `, `+`, `-` or `\` marker. */
export interface FileDiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: string[];
}

/** One changed file; counts are the marked lines of its own hunks. Emitted as `file_diffs` block data. */
export interface FileDiff {
  path: string;
  movePath?: string;
  additions: number;
  deletions: number;
  hunks: FileDiffHunk[];
}

// Mirrors Rust ChatMsg.tool_rich_result; strings are exact source values, absent when unreported.
export interface ToolRichResult {
  kind: "web" | "image";
  origin?: "generated" | "viewed";
  query?: string;
  resultCount?: number;
  action?: string;
  url?: string;
  pattern?: string;
  path?: string;
  status?: string;
  prompt?: string;
}

// Minimal shared chat shape mapped onto Rust ChatMsg. Only id/type/content are contractual; extra fields are allowed.
export interface NormalizedChatMessage
  extends Omit<
    Partial<ChatMsg>,
    "pluginIconSvg" | "inboundKind" | "toolKind" | "toolEdits" | "fileDiffs" | "toolRichResult"
  > {
  id: string;
  type: string;
  content: string;
  /** Monotonic source-order ordinal (host-stamped); stable transcript-sort tiebreaker. */
  seq?: number;
  fromLabel?: string;
  fromTab?: string;
  fromModel?: string;
  fromMesh?: string;
  inboundKind?: "tab" | "report" | "reminder" | "system" | "mesh" | "telegram" | "pebble" | "plugin";
  fromPlugin?: string;
  pluginIconSvg?: string | null;
  timelineMarker?: ChatTimelineMarker;
  sessionIntent?: ChatSessionIntent;
  /** Provider-neutral canonical tool metadata, available on any chat transport. */
  toolKind?: ToolEventKind;
  toolSummary?: string;
  toolDetail?: string;
  toolEdits?: ToolEdit[];
  fileDiffs?: FileDiff[];
  toolRichResult?: ToolRichResult;
  toolError?: boolean;
}

export type ModelInfo = Pick<import("./gen/SerdeModelInfo").SerdeModelInfo, "id" | "displayName"> & Partial<{[K in Exclude<keyof import("./gen/SerdeModelInfo").SerdeModelInfo, "id" | "displayName" | "reasoningEfforts">]: NonNullable<import("./gen/SerdeModelInfo").SerdeModelInfo[K]>}> & {reasoningEfforts?: ReasoningEffortInfo[]};

export type ReasoningEffortInfo = Pick<import("./gen/SerdeReasoningEffortInfo").SerdeReasoningEffortInfo, "id" | "displayName"> & {description?: NonNullable<import("./gen/SerdeReasoningEffortInfo").SerdeReasoningEffortInfo["description"]>};

export type Model = Pick<import("./gen/PluginModel").PluginModel, "id"> & {displayName: NonNullable<import("./gen/PluginModel").PluginModel["displayName"]>} & Partial<Omit<import("./gen/PluginModel").PluginModel, "id" | "displayName">>;

/** Provider-owned grouping of its models, e.g. the upstream company a routed
 *  model belongs to. `id` matches a model-id prefix; `label` is rendered. */
export type ModelGroup = import("./gen/ModelGroup").ModelGroup;

/** Cumulative usage the provider attributes to one model group. snake_case to
 *  match the host wire shape; `cost_micros` is USD × 1_000_000. */
export type ModelGroupUsage = OptionalWireFields<import("./gen/ModelGroupUsage").ModelGroupUsage, "spend_cents" | "spend_limit_cents" | "spend_remaining_cents" | "spend_resets_at_ms">;

export type PresetInfo = OptionalWireFields<import("./gen/PresetInfo").PresetInfo, "description">;

export type SessionMode = "interactive" | "watcher";

export interface ChatWindowConfig {
  maxMessages?: number;
  policy?: "top";
}

export type ObservationConfig = Partial<import("./gen/ObservationConfig").ObservationConfig>;

export type ContextEngineConfig =
  | { kind: "chat"; window?: ChatWindowConfig }
  | (import("./gen/CoreMachineEngine").CoreMachineEngine & {observations?: ObservationConfig});

export type WatcherTickInput = import("./gen/WatcherTickInput").WatcherTickInput;

export type WatcherVerdictStatus = import("./gen/WatcherVerdictStatus").WatcherVerdictStatus;

export type WatcherAction = OptionalWireFields<import("./gen/WatcherAction").WatcherAction, "tab">;

export type WatcherVerdict = OptionalWireFields<import("./gen/WatcherVerdict").WatcherVerdict, "state" | "actions">;

export interface ChatBackendOpenSessionCtx {
  tabId: string;
  config: unknown;
  systemPrompt?: string;
  preset?: string;
  model?: string;
  mode?: SessionMode;
  engine?: ContextEngineConfig;
}

// Live state of an open session (mirrors Rust ChatSessionInfo). `model` absent ⇒ no per-session model.
export type ChatSessionInfo = OptionalWireFields<import("./gen/ChatSessionInfo").ChatSessionInfo, "model">;

// `spawnable` makes the backend user-openable; `watcher` opts into host-driven watcher ticks.
export type ChatBackendCapability = OptionalWireCases<import("./gen/ChatBackendCapability").ChatBackendCapability, "spawnable" | "watcher" | "sessionConfig" | "agentTasks" | "contextOwner">;

export type AgentTaskInput = import("./gen/AgentTaskInput").AgentTaskInput;

export type AgentTaskResult = OptionalWireFields<import("./gen/AgentTaskResult").AgentTaskResult, "error">;

export type ChatBackendCloseReceipt = import("./gen/ChatBackendCloseReceipt").ChatBackendCloseReceipt;

export interface ChatBackend {
  runAgentTask?(input: AgentTaskInput): AgentTaskResult;
  abortAgentTask?(runId: string): void;
  openSession(ctx: ChatBackendOpenSessionCtx): ({sessionId: NonNullable<import("./gen/OpenOut").OpenOut["sessionId"]>});
  sendMessage(sessionId: string, text: string): void;
  // Optional turn-advance, called each tick before `poll` so an async backend drains its tickets. Best-effort; omit if synchronous.
  pump?(sessionId: string): void;
  poll(
    sessionId: string,
    cursor?: string | null,
  ): (Omit<Extract<import("./gen/PollOut").PollOut, {messages: unknown}>, "messages" | "done"> & {messages: NormalizedChatMessage[]; done?: NonNullable<Extract<import("./gen/PollOut").PollOut, {messages: unknown}>["done"]>});
  // Confirmed respawn requires an exact session receipt after all owned processes exit.
  closeSession(sessionId: string): void | ChatBackendCloseReceipt;
  listModels?(): Model[];
  listPresets?(): PresetInfo[];
  // Optional: current live model of a session; omit if the backend tracks none.
  sessionInfo?(sessionId: string): ChatSessionInfo;
  // Optional: switch the model used on the next turn; omit if the backend can't (host surfaces "unsupported").
  setModel?(sessionId: string, model: string): void;
  // Enqueue one machine query; pump/poll streams the completion.
  watcherTick?(sessionId: string, input: WatcherTickInput): void;
}

// ── chatPreprocessor (compose/send-time outbound stage) ──

export interface ChatPreprocessor {
  match: "text";
  chatPreprocess(ctx: import("./gen/PreprocessRequest").PreprocessRequest): ({text: NonNullable<import("./gen/PreprocessOut").PreprocessOut["text"]>}) | null; // null = passthrough
}

// Declared in plugin.json as `capabilities.webhookReceiver`. `audio: true` lets the
// host accept one bounded `audio/*` part and transcribe it with the installed Transcriber.
export type ReceiverStatusQueryCapability = import("./gen/ReceiverStatusQuery").ReceiverStatusQuery;

export type LocalRecordingButtonCapability = import("./gen/LocalRecordingButton").LocalRecordingButton;

export type LocalRecordingTriggerCapability = OptionalWireFields<import("./gen/LocalRecordingTrigger").LocalRecordingTrigger, "button">;

export type WebhookReceiverCapability = OptionalWireFields<import("./gen/WebhookReceiverCap").WebhookReceiverCap, "audio">;

// Metadata only: audio bytes never enter the plugin.
export type WebhookAudio = import("./gen/WebhookAudio").WebhookAudio;

export type WebhookReceiveContext = OptionalWireFields<import("./gen/WebhookReceiveContext").WebhookReceiveContext, "audio">;

export type WebhookDelivery = import("./gen/WebhookDelivery").WebhookDelivery;

export interface WebhookReceiver {
  webhookReceive(ctx: WebhookReceiveContext): WebhookDelivery | null;
}

// ── tabMenu (plugin.json `capabilities.tabMenu`; no export runs on click) ──

// `receiverTarget` requires webhookReceiver or localRecordingTrigger.
export type TabMenuAction = import("./gen/TabMenuAction").TabMenuAction;

export type TabMenuContribution = OptionalWireFields<import("./gen/TabMenuItem").TabMenuItem, "icon">;

// ── surfaces (plugin.json `capabilities.surfaces`; core owns the tab frame) ──

// `view` renders the plugin's `ui/index.html` and requires `capabilities.view`.
export type SurfaceRenderer = import("./gen/SurfaceRenderer").SurfaceRenderer;
export type SurfaceRestore = import("./gen/SurfaceRestore").SurfaceRestore;

/** Host-injected iframe bridge; state subscriptions are hints, not a polling API. */
export interface SurfaceViewBridge {
  surface: { surface: string | null; instance: string | null; tab: string | null };
  invoke(method: string, args?: unknown): Promise<unknown>;
  close(): void;
  onSurfaceStateChanged(callback: () => void): () => void;
}

/** Authoritative context of this registered UI document; no run/budget or private lease. */
export type ViewCallContext = import("./gen/ViewCallContext").ViewCallContext;

export type SurfaceCallContext = import("./gen/SurfaceCallContext").SurfaceCallContext;

export interface SurfaceAgentHandler {
  onSurfaceCall(context: SurfaceCallContext, verb: string, args: string[]): string;
}

export type SurfaceContribution = OptionalWireFields<import("./gen/SurfaceDecl").SurfaceDecl, "agentVerbs" | "stateInvalidation" | "agentBound" | "restore" | "runtime" | "chrome">;

// ── viewHostActions (plugin.json `capabilities.viewHostActions`; requires `view`) ──

// The user-gesture actions the view page may ask for through `ct.host`; each named once.
export type ViewHostAction = import("./gen/ViewHostAction").ViewHostAction;

// `capabilities.viewInputs`, required by (and only allowed with) `pickFile`: what the user may
// pick for the view, as passive media types, each at most `maxBytes`, at most `maxFiles` per pick.
export type ViewInputMediaType = Extract<import("./mediaDelivery").MediaContentType,
  | "image/png" | "image/jpeg" | "image/webp" | "image/gif"
  | "audio/mpeg" | "audio/ogg" | "audio/wav"
  | "video/mp4" | "video/webm" | "video/ogg">;
export type ViewInputsContribution = import("./gen/ViewInputs").ViewInputs;

// One file the user picked: an opaque handle owned by this frame, never a path. Image preview is not
// implied; show it as a file until the host offers a preview URL for it.
export type {PickedInput, PickFileResult, ViewHostResult, ViewHostAvailability, PickFileOptions} from "./viewHost";
import type {PickFileResult, ViewHostResult, ViewHostAvailability, PickFileOptions} from "./viewHost";

// `denied`: no recent activation in this shown, focused view; undeclared; cancelled; `busy` (one
// call at a time, at most one effect per second); `stale` (the frame was hidden or replaced
// meanwhile); or media this frame does not own.
// `unavailable`: the host cannot act here (the web mirror has no file manager).
// `invalid`: malformed text, handle or arguments.
// `window.ct.host` inside the view page. Call the actions from a click or key handler: the host checks
// its own user activation. Media handles are the host-issued ones this frame received; never paths.
export interface ViewHostBridge {
  // Resolves once the host has said what it supports; read it to hide an action, never to probe one.
  availability(): Promise<ViewHostAvailability>;
  copyText(text: string): Promise<ViewHostResult>;
  saveAs(mediaHandle: string, suggestedName?: string): Promise<ViewHostResult>;
  reveal(mediaHandle: string): Promise<ViewHostResult>;
  // The app's own picker (system dialog, or the browser's on the web mirror). `accept` may only narrow
  // the declared `viewInputs.accept`; `multiple` is honored up to `viewInputs.maxFiles`.
  pickFile(options?: PickFileOptions): Promise<PickFileResult>;
}

// ── onAgentCommand (the inverse seam) ──

export type AgentCommandCtx = import("./gen/AgentCommandCtx").AgentCommandCtx;

export interface AgentCommandHandler {
  onAgentCommand(ctx: AgentCommandCtx): ({result: NonNullable<import("./gen/AgentCommandOut").AgentCommandOut["result"]>; catalogueChanged?: import("./gen/AgentCommandOut").AgentCommandOut["catalogueChanged"]}) | ({error: NonNullable<import("./gen/AgentCommandOut").AgentCommandOut["error"]>});
}

// ── The default-export shape: every capability method optional ──

export interface PluginModule
  extends Partial<AgentProvider>,
    Partial<AgentRuntime>,
    Partial<SecretBackend>,
    Partial<Transcriber>,
    Partial<Converter>,
    Partial<import("./blueprintSteps").BlueprintSteps>,
    Partial<DecisionModelCapability>,
    Partial<ChatBackend>,
    Partial<ChatPreprocessor>,
    Partial<WebhookReceiver>,
    Partial<AgentCommandHandler>,
    Partial<SurfaceAgentHandler>,
    Partial<ModelProviderConfig> {
  onPermissionChanged?(event: PermissionChangedEvent): void | import("./gen/HostError").HostError;
  statusBubble?(ctx: TabContext): StatusBubble | null;
  renderGlance?(ctx: TabContext): GlanceView;
  viewCall?: ViewCall;
  /** Requires manifest mediaDelivery. Lookup only: never submit work. Paths are relative to this plugin's own data directory. */
  mediaFile?(id: string, context: MediaFileContext): import("./mediaDelivery").MediaFile;
  /** Refresh account-backed remote execution availability and environments. */
  discoverRemoteTask?(): import("./remoteTasks").RemoteTaskDiscovery;
  glanceAction?(action: string, ctx: TabContext): unknown;
  // Test-only exports and other plugin-private members are allowed.
  [key: string]: unknown;
}

export type PermissionChangedEvent = import("./gen/PermissionChangedEvent").PermissionChangedEvent & {allowed: true};

export type RunMediaLookupContext = import("./gen/RunMediaLookupContext").RunMediaLookupContext;
export type MediaFileContext = ViewCallContext | RunMediaLookupContext;

interface ViewNodeRow extends Omit<Extract<import("./gen/ViewNode").ViewNode, {kind: "row"}>, "children"> {children: ViewNode[]}
interface ViewNodeStack extends Omit<Extract<import("./gen/ViewNode").ViewNode, {kind: "stack"}>, "children"> {children: ViewNode[]}
