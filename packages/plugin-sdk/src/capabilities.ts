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
  starterPrompt?: "no_starter" | "assigned_task" | "idle_wakeup" | "team_bootstrap";
  /** Host-owned text for starter prompts shared across providers. */
  starterPromptText?: string;
  /// Fixed local server the host wants this agent's structured-prompt handle to
  /// listen on. Set only for providers that declare `supportsStructuredPrompts`;
  /// the plugin maps it to the provider's own server flags (opencode:
  /// `--hostname <h> --port <p>`).
  structuredPromptServer?: { hostname: string; port: number };
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
  buildPromptResponse?(optionIndex: number, screen?: string): string[];
  directoryTrustPromptResponse?(prompt: ParsedPrompt, screen?: string): string[] | null;
  parsePrompt?(text: string): ParsedPrompt | null;
  sessionFilePath?(cwd: string, sessionId: string): string | null;
  sessionJsonlPath?(cwd: string, sessionId: string): string | null;
  sessionExists?(cwd: string, sessionId: string): boolean;
  sessionCreatedMs?(cwd: string, sessionId: string): number | null;
  parseSessionDelta?(chunk: string): unknown;
  detectSessionId?(cwd: string, exclude?: string, maxAgeMs?: number): string | null;
  detectActiveSession?(cwd: string, currentSid?: string): string | null;
  detectSessionModel?(cwd: string, sessionId: string): string | null;
  enumerateSessions?(): unknown[];
  describeSession?(filePath: string, head?: string): {
    projectPath?: string;
    title?: string;
    summary?: string;
  } | null;
  parseUsage?(rawText: string, nowMs: number): unknown;
  fetchUsage?(nowMs: number): unknown;
  discoverModels?(): ModelInfo[];
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
}

// ── secretBackend (mirrors the bitwarden plugin + JsSecretBackend) ──

export interface SecretCreds {
  masterPassword?: string;
  email?: string;
  twoFactorToken?: string;
  apiKeyClientId?: string;
  apiKeyClientSecret?: string;
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
  | "generic";

export interface ToolEdit {
  path: string;
  old: string;
  new: string;
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
  inboundKind?: "tab" | "report" | "reminder" | "mesh" | "telegram";
  /** Provider-neutral canonical tool metadata, available on any chat transport. */
  toolKind?: ToolEventKind;
  toolSummary?: string;
  toolDetail?: string;
  toolEdits?: ToolEdit[];
  toolError?: boolean;
}

export interface ModelInfo {
  id: string;
  // camelCase here; the host maps it to Rust ModelInfo.display_name.
  displayName: string;
  description?: string;
  // Account-can-select-this-now. Omitted ⇒ "no info, treat as available"; `false` greys it out. Set by live discovery.
  available?: boolean;
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
    Partial<AgentCommandHandler> {
  statusBubble?(ctx: TabContext): StatusBubble | null;
  renderGlance?(ctx: TabContext): GlanceView;
  viewCall?: ViewCall;
  glanceAction?(action: string, ctx: TabContext): unknown;
  // Test-only exports and other plugin-private members are allowed.
  [key: string]: unknown;
}
