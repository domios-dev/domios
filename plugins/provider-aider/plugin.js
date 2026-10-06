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

// plugins/provider-aider/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  MODE_PROMPT_RE: () => MODE_PROMPT_RE,
  default: () => plugin_default,
  extractBannerModel: () => extractBannerModel,
  lastNonEmptyLine: () => lastNonEmptyLine,
  promptSafeChoice: () => promptSafeChoice
});
module.exports = __toCommonJS(plugin_exports);
var TITLE_RE = /aider/i;
var OUTPUT_MARKERS = ["aider", "Aider", "Aider Chat"];
var OUTPUT_SIGNALS = [
  "Model:",
  "Tokens:",
  "Repo Map:",
  "Run shell command",
  "Add file to the chat",
  "Create new file"
];
var TUI_FRAGMENTS = ["aider", "tokens sent"];
var ASSIGNED_TASK_START_PROMPT = "Execute the assigned task. Follow the configured instructions.";
var IDLE_WAKEUP_START_PROMPT = "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";
var PROMPT_QUESTION_RE = /^(Run shell commands?\?|Add file to the chat\?|Create new file[^\n]*\?)\s*\(Y\)es\/\(N\)o\s*$/;
var SHELL_COMMAND_QUESTION_RE = /^Run shell commands?\?\s*\(Y\)es\/\(N\)o\s*$/;
var COMMAND_LINE_RE = /^[ \t]*[$>][ \t]+(\S[^\n]*)$/;
var MODE_PROMPT_RE = /^(?:[a-z]+)?>\s*$/;
function starterPrompt(params) {
  if (params.starterPromptText) return params.starterPromptText;
  if (params.starterPrompt === "no_starter") return void 0;
  if (params.starterPrompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (params.starterPrompt === "team_bootstrap") throw new Error("team_bootstrap requires host starterPromptText");
  return ASSIGNED_TASK_START_PROMPT;
}
function lastNonEmptyLine(text) {
  const lines = String(text || "").split(/\r?\n/);
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (line) return line;
  }
  return "";
}
function extractBannerModel(text) {
  const match = String(text || "").match(/Model:\s*([^\s]+)/);
  return match ? match[1] : null;
}
function extractPromptedCommands(screen) {
  const lines = String(screen || "").split(/\r?\n/);
  const commands = [];
  for (let i = 0; i < lines.length; i++) {
    const match = lines[i].match(COMMAND_LINE_RE);
    if (match) commands.push(match[1].trim());
  }
  return commands;
}
function isCodetermOnlyCommandList(commands) {
  return commands.length > 0 && commands.every((command) => command.startsWith("codeterm "));
}
function parsePrompt(text) {
  const screen = String(text || "");
  const lines = screen.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const question = lines[i].trim();
    if (!PROMPT_QUESTION_RE.test(question)) continue;
    const options = [["Yes", "1"], ["No", "2"]];
    return { question, options };
  }
  return null;
}
function promptSafeChoice(prompt, screen) {
  if (!prompt || !prompt.question) return null;
  if (!SHELL_COMMAND_QUESTION_RE.test(prompt.question)) return null;
  return isCodetermOnlyCommandList(extractPromptedCommands(screen)) ? "1" : null;
}
var plugin = {
  starterPromptText(prompt) {
    return prompt === "team_bootstrap" ? void 0 : starterPrompt({ starterPrompt: prompt });
  },
  detectFromTitle(title) {
    return TITLE_RE.test(String(title || ""));
  },
  detectFromOutput(text) {
    const t = String(text || "");
    let hasMarker = false;
    for (let i = 0; i < OUTPUT_MARKERS.length; i++) {
      if (t.indexOf(OUTPUT_MARKERS[i]) !== -1) {
        hasMarker = true;
        break;
      }
    }
    if (!hasMarker) return false;
    for (let j = 0; j < OUTPUT_SIGNALS.length; j++) {
      if (t.indexOf(OUTPUT_SIGNALS[j]) !== -1) return true;
    }
    return false;
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
      if (c !== "\u2500" && c !== "\u2501" && c !== "\u2550" && c !== "\u2588" && c !== "\u2580" && c !== " " && c !== "	" && c !== "\n" && c !== "\r") {
        return false;
      }
    }
    return true;
  },
  hasEventMarkers(chunk) {
    return parsePrompt(chunk) !== null;
  },
  detectEvents(text) {
    return parsePrompt(text) ? ["permission_request"] : [];
  },
  discoverModels() {
    return null;
  },
  modelGroupUsage() {
    return [];
  },
  updatePromptSafeChoice(prompt) {
    return promptSafeChoice(prompt, prompt.question);
  },
  parsePrompt,
  buildPromptResponse(optionIndex, _screen) {
    const ENTER = "DQ==";
    const YES = "WQ==";
    const NO = "Tg==";
    return [optionIndex === 0 ? YES : NO, ENTER];
  },
  buildLaunchCommand(params) {
    const p = params || {};
    const parts = ["aider"];
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) {
        parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
      }
    }
    parts.push("--no-auto-commits", "--no-pretty", "--no-fancy-input", "--chat-language", "English");
    const configPath = typeof p.configPath === "string" && p.configPath ? p.configPath : null;
    if (configPath) {
      parts.push("--config", host.shell.quoteFor(configPath, host.platform()));
    }
    const prompt = starterPrompt(p);
    if (p.task && prompt) {
      parts.push("--message", host.shell.quoteFor(prompt, host.platform()));
    }
    return parts.join(" ");
  },
  buildResumeCommand(_sessionId, _skipPermissions) {
    return this.buildLaunchCommand({});
  },
  buildResumeCommandWithContext(params) {
    return this.buildLaunchCommand({ ...params, task: params.systemPrompt || void 0 });
  },
  buildPromptSafeChoice: promptSafeChoice,
  detectSessionId(_cwd, _exclude, _maxAgeMs) {
    return null;
  },
  enumerateSessions() {
    return [];
  },
  findSession(_sessionId) {
    return null;
  },
  sessionExists(_cwd, _sessionId) {
    return false;
  },
  sessionCreatedMs(_cwd, _sessionId) {
    return null;
  },
  detectActiveSession(_cwd, _currentSid) {
    return null;
  },
  detectSessionModel(_cwd, _sessionId) {
    return null;
  },
  usageSources(_cwd, _sessionId) {
    return [];
  },
  usesStructuredChat() {
    return false;
  },
  readStructuredChat(_cwd, _sessionId, cursor) {
    return { messages: [], cursor: cursor || null };
  },
  checkIntegration() {
    return {
      config_file_exists: false,
      plugin_file_exists: false,
      plugin_file_owned: false,
      plugin_file_current: false,
      plugin_installed: false
    };
  },
  installIntegration(_apiPort, _options) {
    return { ok: false, error: "aider has no integration to install" };
  },
  uninstallIntegration() {
    return { ok: true };
  },
  ensureIntegrationOnStartup(_apiPort) {
    return this.checkIntegration();
  }
};
var plugin_default = plugin;
