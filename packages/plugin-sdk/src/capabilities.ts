// Plugin-facing capability contracts. Each capability is one Rust trait the host
// adapts to a `plugin.js` export. Shapes are derived from how the host consumes
// them (src-tauri/src/view/mod.rs for view nodes; the AgentProvider trait; the
// SecretStore/Transcriber adapters) and from the existing hand-written plugins.

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
  [key: string]: unknown;
}

export interface AgentProvider {
  detectFromTitle?(title: string): boolean;
  detectFromOutput?(text: string): boolean;
  screenHasTui?(screen: string): boolean;
  isOutputNoise?(text: string): boolean;
  hasEventMarkers?(chunk: string): boolean;
  detectEvents?(text: string): string[];
  buildLaunchCommand?(params: LaunchParams): string;
  buildResumeCommand?(sessionId: string, skipPermissions?: boolean): string;
  sessionFilePath?(cwd: string, sessionId: string): string | null;
  sessionJsonlPath?(cwd: string, sessionId: string): string | null;
  parseSessionDelta?(chunk: string): unknown;
  detectSessionId?(cwd: string, exclude?: string, maxAgeMs?: number): string | null;
  detectActiveSession?(cwd: string, currentSid?: string): string | null;
  detectSessionModel?(cwd: string, sessionId: string): string | null;
  enumerateSessions?(): unknown[];
  parseUsage?(rawText: string, nowMs: number): unknown;
  parseSessionMessages?(text: string): unknown[];
  checkIntegration?(): unknown;
  installIntegration?(apiPort: number): unknown;
  uninstallIntegration?(): unknown;
  ensureIntegrationOnStartup?(apiPort: number): unknown;
}

// ── secretBackend (mirrors plugins/bitwarden/plugin.js + JsSecretBackend) ──

export interface SecretCreds {
  masterPassword?: string;
  email?: string;
  apiKeyClientId?: string;
  apiKeyClientSecret?: string;
  [key: string]: unknown;
}

export type SecretEnvelope<T = unknown> = { ok: true } | (T & { ok?: true }) | { error: unknown };

export interface SecretBackend {
  secretStatus(): unknown;
  secretUnlock?(creds: SecretCreds): unknown;
  secretLock?(): unknown;
  secretLogout?(): unknown;
  secretGet?(name: string): unknown;
  secretSet?(name: string, value: string): unknown;
  secretList?(): unknown;
  secretDelete?(name: string): unknown;
}

// ── transcriber (mirrors plugins/transcriber/plugin.js + Transcriber trait) ──

export interface TranscribeResult {
  text?: string;
  error?: string;
}

export interface Transcriber {
  transcribe(path: string, lang?: string): TranscribeResult;
  transcribeStatus?(): unknown;
}

// ── The default-export shape: every capability method optional ──

export interface PluginModule
  extends Partial<AgentProvider>,
    Partial<SecretBackend>,
    Partial<Transcriber> {
  statusBubble?(ctx: PaneContext): StatusBubble | null;
  renderGlance?(ctx: PaneContext): GlanceView;
  viewCall?: ViewCall;
  glanceAction?(action: string, ctx: PaneContext): unknown;
  // Test-only exports and other plugin-private members are allowed.
  [key: string]: unknown;
}
