"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// plugins/provider-cursor/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default
});
module.exports = __toCommonJS(plugin_exports);
var TITLE_RE = /cursor/i;
var OUTPUT_FINGERPRINTS = [
  "Workspace Trust Required",
  "Cursor Agent can execute code",
  "Type ? in the prompt bar",
  "Plan, search, build anything"
];
var TUI_FRAGMENTS = [
  "cursor agent",
  "workspace trust required",
  "type ? in the prompt bar",
  "run everything"
];
var plugin = {
  detectFromTitle(title) {
    return TITLE_RE.test(String(title || ""));
  },
  detectFromOutput(text) {
    const t = String(text || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    return t.indexOf("Cursor Agent") !== -1 && t.indexOf("Run Everything") !== -1;
  },
  screenHasTui(screen) {
    const lower = String(screen || "").toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return false;
  },
  isOutputNoise(text) {
    const trimmed = String(text || "").trim();
    if (!trimmed) return true;
    for (let i = 0; i < trimmed.length; i++) {
      const c = trimmed[i];
      if (c !== "\u2500" && c !== "\u2501" && c !== "\u2550" && c !== "\u2502" && c !== "\u2503" && c !== "\u256D" && c !== "\u256E" && c !== "\u256F" && c !== "\u2570" && c !== " " && c !== "	" && c !== "\n" && c !== "\r") {
        return false;
      }
    }
    return true;
  },
  hasEventMarkers(_chunk) {
    return false;
  },
  detectEvents(_text) {
    return [];
  },
  // No `cursor-agent models` list command; curated manifest set, default auto.
  discoverModels() {
    return manifestModels();
  },
  buildLaunchCommand(params) {
    const p = params || {};
    const parts = ["cursor-agent"];
    if (p.skipPermissions) parts.push("--force");
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) {
        parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
      }
    }
    return parts.join(" ");
  },
  buildResumeCommand(sessionId, skipPermissions) {
    const parts = ["cursor-agent", "--resume", host.shell.quote(String(sessionId || ""))];
    if (skipPermissions) parts.push("--force");
    return parts.join(" ");
  },
  // ── Session discovery / chat / usage: CW2 (Phase 2). Valid stubs below. ──
  sessionExists(_cwd, _sessionId) {
    return false;
  },
  sessionCreatedMs(_cwd, _sessionId) {
    return null;
  },
  enumerateSessions() {
    return [];
  },
  detectSessionId(_cwd, _exclude, _maxAgeMs) {
    return null;
  },
  detectActiveSession(_cwd, _currentSid) {
    return null;
  },
  detectSessionModel(_cwd, _sessionId) {
    return null;
  },
  // CW2 decides CLI-replay structured chat vs blob decode; until then the host
  // falls back to the raw xterm view.
  usesStructuredChat() {
    return false;
  },
  parseSessionMessages(_text) {
    return [];
  },
  parseUsage(_rawText, _nowMs) {
    return null;
  }
};
function manifestModels() {
  try {
    const m = JSON.parse(host.manifestJson() || "null");
    return Array.isArray(m && m.models) ? m.models : [];
  } catch (_) {
    return [];
  }
}
var plugin_default = plugin;
