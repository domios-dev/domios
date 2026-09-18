// Plugin-facing capability contracts. Each capability is one Rust trait the host
// adapts to a `plugin.js` export; shapes mirror how the host consumes them.

// ── Shared context ──

export interface TabContext {
  cwd?: string | null;
}

// ── statusBubble ──

export type BubbleTone = "default" | "muted" | "ok" | "warn" | "danger" | "accent";

export interface StatusBubble {
  label: string;
  tone: BubbleTone;
  icon?: string;
  tooltip?: string;
  // "openPanel:view:<id>" opens the plugin's iframe view; "openPanel:<name>" a built-in panel.
  action?: string;
}

// ── glanceView (mirrors src-tauri/src/view/mod.rs ViewNode) ──

export type Tone = "default" | "muted" | "ok" | "warn" | "danger" | "accent";
export type Spacing = "none" | "sm" | "md" | "lg";
export type NoteLevel = "info" | "warn" | "error";

export interface NodeStyle {
  tone?: Tone;
  spacing?: Spacing;
}

export type ViewNode =
  | { kind: "text"; text: string; style?: NodeStyle }
  | { kind: "stat"; label: string; value: string; unit?: string; style?: NodeStyle }
  | { kind: "keyVal"; key: string; value: string; style?: NodeStyle }
  | { kind: "badge"; label: string; tone?: Tone }
  | { kind: "row"; children: ViewNode[]; style?: NodeStyle }
  | { kind: "stack"; children: ViewNode[]; style?: NodeStyle }
  | { kind: "divider" }
  | { kind: "note"; body: string; level?: NoteLevel }
  | { kind: "button"; label: string; action: string; style?: NodeStyle };

export interface GlanceView {
  title?: string;
  nodes: ViewNode[];
}

// ── view (sandboxed iframe UI bridge target) ──

export type ViewCall = (method: string, args: unknown) => unknown;

// ── agentProvider ──
// Method set mirrors plugins/provider-*/plugin.js + the AgentProvider trait.

export interface LaunchParams {
  skipPermissions?: boolean;
  args?: string[];
  agentName?: string;
  sessionId?: string;
  task?: string;
  /** Optional provider-visible starter turn selected by the host. */
  starterPrompt?: "no_starter" | "assigned_task" | "idle_wakeup" | "team_bootstrap" | "startup_handshake";
  /** Host-owned text for starter prompts shared across providers. */
  starterPromptText?: string;
  /// Fixed local server the host wants this agent's structured-prompt handle to
  /// listen on. Set only for providers that declare `supportsStructuredPrompts`;
  /// the plugin maps it to the provider's own server flags (opencode:
  /// `--hostname <h> --port <p>`).
  structuredPromptServer?: { hostname: string; port: number };
  /** Host-minted inert token a provider may carry into its own session record. */
  launchMarker?: string;
  [key: string]: unknown;
}

/** One process in the tab's own PTY tree; `startToken` is its OS process start time. */
export interface LaunchProcess {
  pid: number;
  depth: number;
  startToken: string;
}

/** Everything the host knows about one launch, and nothing about any other. */
export interface LaunchSessionEvidence {
  cwd: string;
  launchMarker?: string | null;
  launchedAtMs: number;
  processes: LaunchProcess[];
}

export interface LaunchSessionDetection {
  sessionId: string;
  source: "pid_registry" | "launch_marker";
}

/** Full launch context for a resumed session, not just its id. */
export interface ResumeParams {
  sessionId: string;
  skipPermissions?: boolean;
  /** Composed system prompt to re-apply; absent when the host has none. */
  systemPrompt?: string | null;
  args?: string[];
  [key: string]: unknown;
}

// picker can't read its layout (e.g. opencode's single-row options).
export interface ParsedPrompt {
  question: string;
  options: [string, string][];
  sections?: Array<{ label: string; active?: boolean; completed?: boolean }>;
  multiSelect?: boolean;
  submitAvailable?: boolean;
  selectedValues?: string[];
}

// ── tab-state classification (mirrors codeterm-plugin tab_state.rs) ──

export type TabState =
  | "working"
  | "waiting_on_auth"
  | "update_prompt"
  | "provider_update_completed"
  | "rate_limited"
  | "clarifying_question"
  | "idle_stuck"
  | "unknown";

/** Host-built snapshot of the tab's OWN session/screen handed to classifyTabState. */
export interface TabStateSnapshot {
  tabId: string;
  agentType: string;
  sessionId?: string | null;
  cwd?: string | null;
  /** Recent rendered screen tail (provider fingerprints match against this). */
  screenText: string;
}

export interface DelayedResume {
  /** ISO-8601 wall-clock time to fire the resume. */
  at: string;
  text: string;
  thenKeys?: string[];
}

/** The action a classifier recommends; the host reactor branches only on these generic fields. */
export interface TabStateAction {
  /** Key names to inject, e.g. ["down", "enter"]. */
  keys?: string[];
  delayedResume?: DelayedResume;
  /** A user-facing surface: `kind` names the card template, `payload` its body. */
  surface?: { kind: string; payload: unknown };
  providerUpdate?: { installKeys: string[]; declineKeys: string[] };
}

export interface TabStateClassification {
  state: TabState;
  /** Ensemble vote score in 0.0..=1.0. */
  confidence: number;
  data?: { resetAt?: string; questions?: string[] };
  action?: TabStateAction;
}

// ── declarative settings schema (mirrors codeterm-plugin provider/settings.rs) ──

/** One field of the declarative settings UI, tagged by `kind` (section, toggle, text, …). */
export interface SettingField {
  kind: string;
  [key: string]: unknown;
}

// ── provider self-update (mirrors codeterm-plugin provider/capabilities.rs) ──

export interface ProviderSelfUpdateCapability {
  inSession: boolean;
  restartRequired: boolean;
  completionSignal?: "provider_classification";
}

export interface AgentProvider {
  starterPromptText?(prompt: "no_starter" | "assigned_task" | "idle_wakeup" | "team_bootstrap"): string | undefined;
  detectFromTitle?(title: string): boolean;
  detectFromOutput?(text: string): boolean;
  screenHasTui?(screen: string): boolean;
  isOutputNoise?(text: string): boolean;
  hasEventMarkers?(chunk: string): boolean;
  detectEvents?(text: string): string[];
  extractSessionName?(screen: string): string | null;
  buildLaunchCommand?(params: LaunchParams): string;
  buildResumeCommand?(sessionId: string, skipPermissions?: boolean): string;
  /** Implement only when a resumed session drops the launch-time system prompt. */
  buildResumeCommandWithContext?(params: ResumeParams): string;
  buildPromptResponse?(optionIndex: number, screen?: string): string[];
  /** Directory-trust dialog on the live screen: one base64 key, `[]` to hold it, `null` when none is shown. */
  directoryTrustPromptResponse?(screen: string): string[] | null;
  /** Tool-permission approval for the hook-reported tool; `null` leaves the prompt to the user. */
  permissionPromptResponse?(
    toolName: string,
    prompt: ParsedPrompt,
    screen?: string,
  ): { optionIndex: number; rule: string; steps: string[] } | null;
  parsePrompt?(text: string): ParsedPrompt | null;
  sessionFilePath?(cwd: string, sessionId: string): string | null;
  sessionJsonlPath?(cwd: string, sessionId: string): string | null;
  /** Further usage logs owned by this exact session file (e.g. sub-agent transcripts beside it). */
  sessionUsageSources?(sessionFilePath: string, sessionId: string): string[];
  sessionExists?(cwd: string, sessionId: string): boolean;
  sessionCreatedMs?(cwd: string, sessionId: string): number | null;
  /** Each returned row carries `recordIndex`, the chunk line it was parsed from. */
  parseSessionDelta?(chunk: string): unknown;
  detectSessionId?(cwd: string, exclude?: string, maxAgeMs?: number): string | null;
  /** Session named by this launch's own provider record; null when none names it. */
  detectLaunchSession?(evidence: LaunchSessionEvidence): LaunchSessionDetection | null;
  detectActiveSession?(cwd: string, currentSid?: string): string | null;
  detectSessionModel?(cwd: string, sessionId: string): string | null;
  detectSessionReasoningEffort?(cwd: string, sessionId: string): string | null;
  enumerateSessions?(): unknown[];
  /** The one record owning exactly this id, or null; never scans session history. */
  findSession?(sessionId: string): unknown | null;
  describeSession?(filePath: string, head?: string): {
    projectPath?: string;
    title?: string;
    summary?: string;
  } | null;
  parseUsage?(rawText: string, nowMs: number): unknown;
  fetchUsage?(nowMs: number): unknown;
  discoverModels?(): ModelInfo[] | null;
  parseSessionMessages?(text: string): unknown[];
  usesStructuredChat?(): boolean;
  readStructuredChat?(cwd: string, sessionId: string, cursor?: string | null): unknown;
  checkIntegration?(): unknown;
  installIntegration?(
    apiPort: number,
    options: { consent: boolean; precompact_memory_export?: boolean },
  ): unknown;
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
  providerSelfUpdateCapability?(): ProviderSelfUpdateCapability | null;
}

// ── secretBackend (mirrors the bitwarden plugin + JsSecretBackend) ──

export interface SecretCreds {
  masterPassword?: string;
  email?: string;
  twoFactorToken?: string;
  apiKeyClientId?: string;
  apiKeyClientSecret?: string;
  /** Explicit consent to remember this credential for headless unlock. The host
   *  always sends a bool; treat absent or malformed as false. */
  persistForAutoUnlock: boolean;
  [key: string]: unknown;
}

// core SecretStore trait: `{ ok }` / `{ ok: <payload> }` / `{ error: <kind> }`.
export type SecretError = { kind: string; message?: string; candidates?: string[] };
export type SecretEnvelope<T = unknown> = { ok: T } | { error: SecretError };

// secretStatus is the one method that returns a bare StoreStatus, not an envelope.
export interface SecretStatus {
  status: "unlocked" | "locked" | "logged_out" | "unavailable";
  user?: string | null;
  transient?: boolean;
  endpoint?: string;
  reason?: string;
}

export interface SecretSaveRequest {
  name: string;
  value: string;
  notes?: string | null;
  folder?: string | null;
  organization?: string | null;
  collection?: string | null;
  overwrite?: boolean;
}

export interface SecretListFilter {
  organization?: string;
  collection?: string;
  folder?: string;
  limit?: number;
}

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

export interface TranscribeResult {
  text?: string;
  error?: string;
}

export interface Transcriber {
  transcribe(path: string, lang?: string): TranscribeResult;
  transcribeStatus?(): unknown;
}

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

export type ChatTimelineMarker =
  | { kind: "queued" }
  | { kind: "external_edit"; path: string }
  | { kind: "context_compaction" };

export interface ChatSessionIntent {
  title?: string;
  goal?: string;
}

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
export interface NormalizedChatMessage {
  id: string;
  type: string;
  content: string;
  /** Monotonic source-order ordinal (host-stamped); stable transcript-sort tiebreaker. */
  seq?: number;
  fromLabel?: string;
  fromTab?: string;
  fromModel?: string;
  fromMesh?: string;
  inboundKind?: "tab" | "report" | "reminder" | "mesh" | "telegram" | "pebble" | "plugin";
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

export interface ModelInfo {
  id: string;
  // camelCase here; the host maps it to Rust ModelInfo.display_name.
  displayName: string;
  description?: string;
  // Account-can-select-this-now. Omitted ⇒ "no info, treat as available"; `false` greys it out. Set by live discovery.
  available?: boolean;
  // A probe that could not run reports "unknown" here and publishes no
  // `available` boolean: unavailable must mean the CLI answered no.
  availability?: "available" | "unavailable" | "unknown";
  // Marks this model to lead the picker list, ahead of the rest.
  featured?: boolean;
  // Short chip rendered beside the model label in spawn pickers.
  badge?: string;
}

export interface Model extends ModelInfo {
}

export interface PresetInfo {
  id: string;
  name: string;
  description?: string;
}

export type SessionMode = "interactive" | "watcher";

export interface ChatWindowConfig {
  maxMessages?: number;
  policy?: "top";
}

export interface ObservationConfig {
  orchestration?: boolean;
  chatTail?: number;
  reports?: boolean;
}

export type ContextEngineConfig =
  | { kind: "chat"; window?: ChatWindowConfig }
  | {
      kind: "machine";
      charter: string;
      observations?: ObservationConfig;
      tickIntervalSecs?: number;
    };

export interface WatcherTickInput {
  tick: number;
  nowMs: number;
  state: unknown;
  observations?: unknown;
}

export type WatcherVerdictStatus = "ok" | "attention" | "stalled";

export interface WatcherAction {
  kind: "nudge" | "notify" | "report";
  tab?: string;
  message: string;
}

export interface WatcherVerdict {
  status: WatcherVerdictStatus;
  summary: string;
  state?: unknown;
  actions?: WatcherAction[];
}

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
export interface ChatSessionInfo {
  model?: string;
}

// `spawnable` makes the backend user-openable; `watcher` opts into host-driven watcher ticks.
export type ChatBackendCapability = boolean | { spawnable?: boolean; watcher?: boolean };

export interface ChatBackend {
  openSession(ctx: ChatBackendOpenSessionCtx): { sessionId: string };
  sendMessage(sessionId: string, text: string): void;
  // Optional turn-advance, called each tick before `poll` so an async backend drains its tickets. Best-effort; omit if synchronous.
  pump?(sessionId: string): void;
  poll(
    sessionId: string,
    cursor?: string | null,
  ): { messages: NormalizedChatMessage[]; cursor: string | null; done?: boolean };
  closeSession(sessionId: string): void;
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
  chatPreprocess(ctx: {
    tabId: string;
    cwd: string | null;
    provider: string | null;
    text: string;
    turnIndex: number;
  }): { text: string } | null; // null = passthrough
}

// Declared in plugin.json as `capabilities.webhookReceiver`. `audio: true` lets the
// host accept one bounded `audio/*` part and transcribe it with the installed Transcriber.
export interface WebhookReceiverCapability {
  kind: "pebble";
  audio?: boolean;
}

// Metadata only: audio bytes never enter the plugin.
export interface WebhookAudio {
  mediaType: string;
  size: number;
}

export interface WebhookReceiveContext {
  payload: unknown;
  receivedAt: string;
  // Present only for an audio-capable receiver that was sent an audio part.
  audio?: WebhookAudio;
}

export interface WebhookDelivery {
  text: string;
  eventId?: string;
  // The sender's own transcript; the host renders it labelled beside its own transcription.
  transcript?: string;
}

export interface WebhookReceiver {
  webhookReceive(ctx: WebhookReceiveContext): WebhookDelivery | null;
}

// ── tabMenu (plugin.json `capabilities.tabMenu`; no export runs on click) ──

// Closed set of host-owned actions. `receiverTarget` requires `webhookReceiver`.
export type TabMenuAction = "receiverTarget";

export interface TabMenuContribution {
  // 1-32 lowercase letters, digits or '-', unique within the plugin.
  id: string;
  // 1-48 trimmed characters; the item's accessible name.
  label: string;
  action: TabMenuAction;
  /** Optional relative path to an SVG shipped inside the plugin bundle. */
  icon?: string;
}

// ── onAgentCommand (the inverse seam) ──

export interface AgentCommandCtx {
  sessionId: string;
  verb: string;
  args: string[];
}

export interface AgentCommandHandler {
  onAgentCommand(ctx: AgentCommandCtx): { result: string } | { error: string };
}

// ── The default-export shape: every capability method optional ──

export interface PluginModule
  extends Partial<AgentProvider>,
    Partial<SecretBackend>,
    Partial<Transcriber>,
    Partial<ChatBackend>,
    Partial<ChatPreprocessor>,
    Partial<WebhookReceiver>,
    Partial<AgentCommandHandler> {
  statusBubble?(ctx: TabContext): StatusBubble | null;
  renderGlance?(ctx: TabContext): GlanceView;
  viewCall?: ViewCall;
  glanceAction?(action: string, ctx: TabContext): unknown;
  // Test-only exports and other plugin-private members are allowed.
  [key: string]: unknown;
}
