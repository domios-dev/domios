// Plugin-facing capability contracts. Each capability is one Rust trait the host
// adapts to a `plugin.js` export; shapes mirror how the host consumes them.

// ── Shared context ──

export interface PaneContext {
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
  /// Fixed local server the host wants this agent's structured-prompt handle to
  /// listen on. Set only for providers that declare `supportsStructuredPrompts`;
  /// the plugin maps it to the provider's own server flags (opencode:
  /// `--hostname <h> --port <p>`).
  structuredPromptServer?: { hostname: string; port: number };
  [key: string]: unknown;
}

// A permission prompt the provider parsed itself when the host's universal
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
  parseUsage?(rawText: string, nowMs: number): unknown;
  discoverModels?(): ModelInfo[];
  parseSessionMessages?(text: string): unknown[];
  usesStructuredChat?(): boolean;
  readStructuredChat?(cwd: string, sessionId: string, cursor?: string | null): unknown;
  checkIntegration?(): unknown;
  installIntegration?(apiPort: number): unknown;
  uninstallIntegration?(): unknown;
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

// Most secret ops return an envelope the JsSecretBackend adapter maps onto the
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
// A plugin that *is* a pane's chat source/transport. The host adapter calls these
// via call_json (mirror src-tauri/src/plugin/js_runtime/transcriber.rs) and pushes
// `poll` output into the existing StructuredSink / `pane-chat-msg` path.

// Minimal shared chat shape mapped onto Rust ChatMsg. Only id/type/content are contractual; extra fields are allowed.
export interface NormalizedChatMessage {
  id: string;
  type: string;
  content: string;
  /** Monotonic source-order ordinal (host-stamped); stable transcript-sort tiebreaker. */
  seq?: number;
  fromLabel?: string;
  fromPane?: string;
  fromModel?: string;
  fromMesh?: string;
  inboundKind?: "pane" | "report" | "reminder" | "mesh" | "telegram";
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

export interface ChatBackendOpenSessionCtx {
  paneId: string;
  config: unknown;
  systemPrompt?: string;
  preset?: string;
  model?: string;
}

// Live state of an open session (mirrors Rust ChatSessionInfo). `model` absent ⇒ no per-session model.
export interface ChatSessionInfo {
  model?: string;
}

// Manifest form for `capabilities.chatBackend`. Bare `true` = registered but not
// user-spawnable (host-/agent-driven); `{ spawnable: true }` = user-openable in the picker.
export type ChatBackendCapability = boolean | { spawnable?: boolean };

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
}

// ── chatPreprocessor (compose/send-time outbound stage) ──
// A stage in the outbound message pipeline. `match` is the slot it grabs; two
// stages claiming the same slot are flagged as a conflict at install time.

export interface ChatPreprocessor {
  match: "text";
  chatPreprocess(ctx: {
    paneId: string;
    cwd: string | null;
    provider: string | null;
    text: string;
    turnIndex: number;
  }): { text: string } | null; // null = passthrough
}

// ── onAgentCommand (the inverse seam) ──
// A spawned agent calls `codeterm plugin <id> <verb> …`; the socket dispatch routes
// `plugin:<id>.<verb>` to the owning plugin's handler (requires the agent_commands grant).

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
  statusBubble?(ctx: PaneContext): StatusBubble | null;
  renderGlance?(ctx: PaneContext): GlanceView;
  viewCall?: ViewCall;
  glanceAction?(action: string, ctx: PaneContext): unknown;
  // Test-only exports and other plugin-private members are allowed.
  [key: string]: unknown;
}
