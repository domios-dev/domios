// Cursor (cursor-agent) provider plugin (runtime logic, capability: agentProvider).
// Authored in TypeScript against @codeterm/plugin-sdk and compiled to a
// QuickJS-compatible plugin.js by scripts/build-plugin.mjs. Phase 1 scope:
// detect fingerprints + launch/resume command construction. Session discovery,
// structured chat and usage parsing are CW2 — valid stubs here so the plugin
// loads. The frontend ProviderPlugin contract lives in the separate src/index.ts.
import type { LaunchParams, ModelInfo, PluginModule } from "@codeterm/plugin-sdk";

const TITLE_RE = /cursor/i;
// Structural fingerprints of the cursor-agent TUI (rendered screen), captured
// from a live session. These are functional UI strings (trust gate, banner
// hint, force-mode status label), not marketing copy that vendors churn.
const OUTPUT_FINGERPRINTS = [
  "Workspace Trust Required",
  "Cursor Agent can execute code",
  "Type ? in the prompt bar",
  "Plan, search, build anything",
];
const TUI_FRAGMENTS = [
  "cursor agent",
  "workspace trust required",
  "type ? in the prompt bar",
  "run everything",
];

const plugin: PluginModule = {
  detectFromTitle(title: string): boolean {
    return TITLE_RE.test(String(title || ""));
  },

  detectFromOutput(text: string): boolean {
    const t = String(text || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    // Banner app-name plus the force-mode status label is a two-signal match
    // that a stray "Cursor Agent" mention in a shell can't reach alone.
    return t.indexOf("Cursor Agent") !== -1 && t.indexOf("Run Everything") !== -1;
  },

  screenHasTui(screen: string): boolean {
    const lower = String(screen || "").toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return false;
  },

  isOutputNoise(text: string): boolean {
    const trimmed = String(text || "").trim();
    if (!trimmed) return true;
    for (let i = 0; i < trimmed.length; i++) {
      const c = trimmed[i];
      if (
        c !== "─" && c !== "━" && c !== "═" && c !== "│" && c !== "┃" &&
        c !== "╭" && c !== "╮" && c !== "╯" && c !== "╰" &&
        c !== " " && c !== "\t" && c !== "\n" && c !== "\r"
      ) {
        return false;
      }
    }
    return true;
  },

  hasEventMarkers(_chunk: string): boolean {
    return false;
  },

  detectEvents(_text: string): string[] {
    return [];
  },

  // No `cursor-agent models` list command; curated manifest set, default auto.
  discoverModels(): ModelInfo[] {
    return manifestModels();
  },

  buildLaunchCommand(params: LaunchParams): string {
    const p = params || {};
    const parts: string[] = ["cursor-agent"];
    if (p.skipPermissions) parts.push("--force");
    if (p.args && p.args.length > 0) {
      // Quote each arg — caller-supplied via the spawn API; prevents shell injection.
      for (let i = 0; i < p.args.length; i++) {
        parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
      }
    }
    return parts.join(" ");
  },

  buildResumeCommand(sessionId: string, skipPermissions?: boolean): string {
    const parts = ["cursor-agent", "--resume", host.shell.quote(String(sessionId || ""))];
    if (skipPermissions) parts.push("--force");
    return parts.join(" ");
  },

  // ── Session discovery / chat / usage: CW2 (Phase 2). Valid stubs below. ──

  sessionExists(_cwd: string, _sessionId: string): boolean {
    return false;
  },

  sessionCreatedMs(_cwd: string, _sessionId: string): number | null {
    return null;
  },

  enumerateSessions(): unknown[] {
    return [];
  },

  detectSessionId(_cwd: string, _exclude?: string, _maxAgeMs?: number): string | null {
    return null;
  },

  detectActiveSession(_cwd: string, _currentSid?: string): string | null {
    return null;
  },

  detectSessionModel(_cwd: string, _sessionId: string): string | null {
    return null;
  },

  // CW2 decides CLI-replay structured chat vs blob decode; until then the host
  // falls back to the raw xterm view.
  usesStructuredChat(): boolean {
    return false;
  },

  parseSessionMessages(_text: string): unknown[] {
    return [];
  },

  parseUsage(_rawText: string, _nowMs: number): unknown {
    return null;
  },
};

interface CursorManifest { models?: ModelInfo[] }

function manifestModels(): ModelInfo[] {
  try {
    const m = JSON.parse(host.manifestJson() || "null") as CursorManifest | null;
    return Array.isArray(m && m.models) ? m!.models! : [];
  } catch (_) {
    return [];
  }
}

export default plugin;
