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
  deliverText: () => deliverText,
  extractBannerModel: () => extractBannerModel,
  lastNonEmptyLine: () => lastNonEmptyLine,
  parsePromptWithContext: () => parsePromptWithContext,
  pickTaggedBlockTag: () => pickTaggedBlockTag,
  promptSafeChoice: () => promptSafeChoice,
  wrapMultiline: () => wrapMultiline
});
module.exports = __toCommonJS(plugin_exports);

// plugins/provider-aider/src/history.ts
var THINKING_BLOCK_RE = /<thinking-content-[^>]*>[\s\S]*?<\/thinking-content-[^>]*>/g;
var META_LINE_RE = /^> /;
function parseAiderHistoryDelta(chunk) {
  const text = String(chunk || "");
  const lines = text.split(/\r?\n/);
  const messages = [];
  let currentRole = null;
  let currentText = [];
  let currentStart = 0;
  function flush() {
    if (currentRole && currentText.length > 0) {
      let body = currentText.join("\n").trim();
      if (currentRole === "assistant") {
        body = body.replace(THINKING_BLOCK_RE, "").trim();
        body = body.split(/\r?\n/).filter((line) => !META_LINE_RE.test(line)).join("\n").trim();
      }
      if (body) {
        messages.push({
          role: currentRole,
          blocks: [{ kind: "text", data: { text: body } }],
          ts: null,
          recordIndex: currentStart
        });
      }
    }
    currentText = [];
  }
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (line.startsWith("#### ")) {
      flush();
      const userText = line.slice(5).trim();
      if (!userText || userText === "<blank>") {
        currentRole = null;
        continue;
      }
      currentRole = "user";
      currentStart = i;
      currentText.push(userText);
      continue;
    }
    if (currentRole === null) continue;
    if (currentRole === "user" && line.trim()) {
      flush();
      currentRole = "assistant";
      currentStart = i;
    }
    currentText.push(line);
  }
  flush();
  return { messages };
}

// plugins/provider-aider/src/plugin.ts
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
var PROMPT_QUESTION_RE = /^(Run shell commands?\?|Add file to the chat\?|Create new file[^\n]*\?|Add \.aider\* to \.gitignore \(recommended\)\?|Open documentation url for more info\?)\s*\(Y\)es\/\(N\)o(?:\/\(D\)on't ask again)?\s*(\[[^\]]*\])?\s*:?\s*$/;
var SHELL_COMMAND_QUESTION_RE = /^Run shell commands?\?\s*\(Y\)es\/\(N\)o\s*$/;
var GITIGNORE_QUESTION_RE = /^Add \.aider\* to \.gitignore \(recommended\)\?\s*\(Y\)es\/\(N\)o\s*(\[[^\]]*\])?\s*:?\s*$/;
var DOCS_URL_QUESTION_RE = /^Open documentation url for more info\?\s*\(Y\)es\/\(N\)o\/\(D\)on't ask again\s*(\[[^\]]*\])?\s*:?\s*$/;
var COMMAND_LINE_RE = /^[ \t]*[$>][ \t]+(\S[^\n]*)$/;
var MODE_PROMPT_RE = /^(?:[a-z]+)?>\s*$/;
var TAGGED_BLOCK_BASE = "domios";
function isMultiline(text) {
  return /\r?\n/.test(String(text || ""));
}
function pickTaggedBlockTag(text) {
  let tag = TAGGED_BLOCK_BASE;
  let suffix = 1;
  while (text.includes(`{${tag}`) || text.includes(`{/${tag}}`)) {
    tag = `${TAGGED_BLOCK_BASE}${suffix}`;
    suffix += 1;
  }
  return tag;
}
function wrapMultiline(text) {
  const body = String(text || "");
  const tag = pickTaggedBlockTag(body);
  return `{${tag}
${body}
${tag}}`;
}
function deliverText(text) {
  return isMultiline(text) ? wrapMultiline(text) : String(text || "");
}
function pluginSettings() {
  const raw = typeof host.settingsJson === "function" ? host.settingsJson() : null;
  if (!raw) return {};
  try {
    const parsed = typeof raw === "string" ? JSON.parse(raw) : raw;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}
function withLaunchEnv(command, p) {
  const platform = host.platform();
  const quote = (v) => host.shell.quoteFor(v, platform);
  const isPowerShell = platform === "windows";
  const settings = pluginSettings();
  const exports = [];
  const nonce = typeof p.sessionId === "string" && p.sessionId || typeof p.launchMarker === "string" && p.launchMarker || null;
  if (nonce) {
    exports.push(
      isPowerShell ? `$env:CODETERM_SESSION_BINDING_NONCE=${quote(nonce)};` : `export CODETERM_SESSION_BINDING_NONCE=${quote(nonce)};`
    );
  }
  const apiBase = typeof settings.apiBase === "string" && settings.apiBase || typeof p.apiBase === "string" && p.apiBase || null;
  if (apiBase) {
    exports.push(
      isPowerShell ? `$env:OPENAI_API_BASE=${quote(apiBase)};` : `export OPENAI_API_BASE=${quote(apiBase)};`
    );
  }
  const apiKeySecret = typeof settings.apiKeySecret === "string" && settings.apiKeySecret || typeof p.apiKeySecret === "string" && p.apiKeySecret || null;
  if (apiKeySecret) {
    exports.push(
      isPowerShell ? `$env:OPENAI_API_KEY=(codeterm mem secret get --name ${quote(apiKeySecret)});` : `export OPENAI_API_KEY="$(codeterm mem secret get --name ${quote(apiKeySecret)})";`
    );
  }
  if (exports.length === 0) return command;
  return `${exports.join(" ")} ${command}`;
}
function starterPrompt(params) {
  if (params.starterPromptText) return params.starterPromptText;
  if (params.starterPrompt === "no_starter") return void 0;
  if (params.starterPrompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (params.starterPrompt === "team_bootstrap") throw new Error("team_bootstrap requires host starterPromptText");
  return ASSIGNED_TASK_START_PROMPT;
}
function historyDir(cwd) {
  return `${String(cwd || "")}/.aider/history`;
}
function historyFilePath(cwd, sessionId) {
  return `${historyDir(cwd)}/${sessionId}.md`;
}
function historyFileEntries(cwd) {
  const fs = globalThis.host?.fs;
  if (!fs || typeof fs.readDir !== "function") return [];
  let entries;
  try {
    entries = fs.readDir(historyDir(cwd));
  } catch (_) {
    return [];
  }
  const out = [];
  for (const raw of entries) {
    const entry = raw;
    if (!entry || typeof entry.name !== "string") continue;
    if (!entry.name.endsWith(".md")) continue;
    const id = entry.name.slice(0, -3);
    if (!id) continue;
    out.push({
      id,
      path: typeof entry.path === "string" && entry.path ? entry.path : historyFilePath(cwd, id),
      modifiedMs: typeof entry.modifiedMs === "number" ? entry.modifiedMs : null
    });
  }
  return out;
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
function isStartupDialog(question) {
  return GITIGNORE_QUESTION_RE.test(question) || DOCS_URL_QUESTION_RE.test(question);
}
function parsePrompt(text) {
  const screen = String(text || "");
  const lines = screen.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const question = lines[i].trim();
    if (!PROMPT_QUESTION_RE.test(question)) continue;
    if (isStartupDialog(question)) continue;
    const options = [["Yes", "1"], ["No", "2"]];
    return { question, options };
  }
  return null;
}
function parsePromptWithContext(text) {
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
  if (GITIGNORE_QUESTION_RE.test(prompt.question)) return "1";
  if (DOCS_URL_QUESTION_RE.test(prompt.question)) return "3";
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
    const text = String(screen || "");
    const lower = text.toLowerCase();
    let hasFragment = false;
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) {
        hasFragment = true;
        break;
      }
    }
    const prompt = lastNonEmptyLine(text);
    if (!MODE_PROMPT_RE.test(prompt)) return false;
    if (/^[a-z]+>$/.test(prompt)) return true;
    return hasFragment && /aider/i.test(text);
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
    return parsePrompt(chunk) !== null || parsePromptWithContext(chunk) !== null;
  },
  detectEvents(text) {
    return parsePrompt(text) || parsePromptWithContext(text) ? ["permission_request"] : [];
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
    const DONT_ASK = "RA==";
    if (optionIndex === 2) return [DONT_ASK, ENTER];
    return [optionIndex === 0 ? YES : NO, ENTER];
  },
  buildLaunchCommand(params) {
    const p = params || {};
    const platform = host.platform();
    const quote = (v) => host.shell.quoteFor(v, platform);
    const isPowerShell = platform === "windows";
    const cwd = typeof p.cwd === "string" && p.cwd ? p.cwd : ".";
    const historyDirPath = `${cwd}/.aider/history`;
    const escapeDir = (value) => isPowerShell ? value.replace(/`/g, "``").replace(/"/g, '`"') : value.replace(/(["\\$`])/g, "\\$1");
    const dirQuoted = escapeDir(historyDirPath);
    const chatHistoryPath = isPowerShell ? `"${dirQuoted}/$($env:CODETERM_SESSION_BINDING_NONCE).md"` : `"${dirQuoted}/\${CODETERM_SESSION_BINDING_NONCE}.md"`;
    const inputHistoryPath = isPowerShell ? `"${dirQuoted}/$($env:CODETERM_SESSION_BINDING_NONCE).input"` : `"${dirQuoted}/\${CODETERM_SESSION_BINDING_NONCE}.input"`;
    const parts = ["aider"];
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) {
        parts.push(quote(String(p.args[i])));
      }
    }
    parts.push("--no-auto-commits", "--no-pretty", "--no-fancy-input", "--no-show-model-warnings", "--chat-language", "English");
    const settings = pluginSettings();
    const configPath = typeof settings.configPath === "string" && settings.configPath || typeof p.configPath === "string" && p.configPath || null;
    if (configPath) {
      parts.push("--config", quote(configPath));
    }
    const prompt = starterPrompt(p);
    if (p.task && prompt) {
      parts.push("--message", quote(prompt));
    }
    const command = parts.join(" ");
    const historyFlags = [
      "--chat-history-file",
      chatHistoryPath,
      "--input-history-file",
      inputHistoryPath
    ].join(" ");
    const mkdir = isPowerShell ? `New-Item -ItemType Directory -Force -Path ${quote(historyDirPath)} | Out-Null;` : `mkdir -p ${quote(historyDirPath)};`;
    return withLaunchEnv(`${mkdir} ${command} ${historyFlags}`, p);
  },
  buildResumeCommand(_sessionId, _skipPermissions) {
    return this.buildLaunchCommand({});
  },
  buildResumeCommandWithContext(params) {
    return this.buildLaunchCommand({ ...params, task: params.systemPrompt || void 0 });
  },
  buildPromptSafeChoice: promptSafeChoice,
  launchOnboardingResponse(screen) {
    const prompt = parsePromptWithContext(String(screen || ""));
    if (!prompt) return null;
    const choice = promptSafeChoice(prompt, String(screen || ""));
    if (!choice) return null;
    const keys = this.buildPromptResponse(Number(choice) - 1, String(screen || ""));
    return { frameKey: prompt.question, step: keys[0] };
  },
  launchOnboardingSafeChoice(screen) {
    const prompt = parsePromptWithContext(String(screen || ""));
    return prompt ? promptSafeChoice(prompt, String(screen || "")) : null;
  },
  chatPreprocess(ctx) {
    if (ctx?.provider !== "aider") return null;
    const text = String(ctx?.text || "");
    const delivered = deliverText(text);
    return delivered === text ? null : { text: delivered };
  },
  detectSessionId(cwd, exclude, maxAgeMs) {
    const excluded = Array.isArray(exclude) ? exclude : exclude ? [exclude] : [];
    const limit = typeof maxAgeMs === "number" && maxAgeMs > 0 ? maxAgeMs : 0;
    const now = Date.now();
    let newest = null;
    for (const entry of historyFileEntries(cwd)) {
      if (excluded.indexOf(entry.id) !== -1) continue;
      if (limit > 0) {
        if (entry.modifiedMs === null) continue;
        if (now - entry.modifiedMs > limit) continue;
      }
      if (!newest) {
        newest = entry;
        continue;
      }
      const a = typeof entry.modifiedMs === "number" ? entry.modifiedMs : -1;
      const b = typeof newest.modifiedMs === "number" ? newest.modifiedMs : -1;
      if (a > b) newest = entry;
    }
    return newest ? newest.id : null;
  },
  detectLaunchSession(evidence) {
    const marker = evidence && typeof evidence.launchMarker === "string" ? evidence.launchMarker : "";
    if (!marker) return null;
    const cwd = evidence && typeof evidence.cwd === "string" ? evidence.cwd : "";
    if (!cwd) return null;
    return historyFileEntries(cwd).some((entry) => entry.id === marker) ? { sessionId: marker, source: "launch_marker" } : null;
  },
  enumerateSessions() {
    return [];
  },
  findSession(_sessionId) {
    return null;
  },
  sessionExists(cwd, sessionId) {
    if (!sessionId) return false;
    return historyFileEntries(cwd).some((entry) => entry.id === sessionId);
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
  sessionFilePath(cwd, sessionId) {
    if (!cwd || !sessionId) return null;
    return historyFilePath(cwd, sessionId);
  },
  sessionJsonlPath(cwd, sessionId) {
    if (!cwd || !sessionId) return null;
    return historyFilePath(cwd, sessionId);
  },
  parseSessionDelta(chunk) {
    return parseAiderHistoryDelta(String(chunk || ""));
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
