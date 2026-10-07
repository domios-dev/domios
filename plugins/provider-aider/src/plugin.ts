import type {
  LaunchParams,
  ModelInfo,
  ParsedPrompt,
  PluginModule,
  ResumeParams,
} from "@codeterm/plugin-sdk";
import {
  parseAiderHistoryDelta,
} from "./history";

const TITLE_RE = /aider/i;
const OUTPUT_MARKERS = ["aider", "Aider", "Aider Chat"];
const OUTPUT_SIGNALS = [
  "Model:",
  "Tokens:",
  "Repo Map:",
  "Run shell command",
  "Add file to the chat",
  "Create new file",
];
const TUI_FRAGMENTS = ["aider", "tokens sent"];
const ASSIGNED_TASK_START_PROMPT = "Execute the assigned task. Follow the configured instructions.";
const IDLE_WAKEUP_START_PROMPT = "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";
const PROMPT_QUESTION_RE = /^(Run shell commands?\?|Add file to the chat\?|Create new file[^\n]*\?|Add \.aider\* to \.gitignore \(recommended\)\?|Open documentation url for more info\?)\s*\(Y\)es\/\(N\)o(?:\/\(D\)on't ask again)?\s*(\[[^\]]*\])?\s*:?\s*$/;
const SHELL_COMMAND_QUESTION_RE = /^Run shell commands?\?\s*\(Y\)es\/\(N\)o\s*$/;
const GITIGNORE_QUESTION_RE = /^Add \.aider\* to \.gitignore \(recommended\)\?\s*\(Y\)es\/\(N\)o\s*(\[[^\]]*\])?\s*:?\s*$/;
const DOCS_URL_QUESTION_RE = /^Open documentation url for more info\?\s*\(Y\)es\/\(N\)o\/\(D\)on't ask again\s*(\[[^\]]*\])?\s*:?\s*$/;
const COMMAND_LINE_RE = /^[ \t]*[$>][ \t]+(\S[^\n]*)$/;
const MODE_PROMPT_RE = /^(?:[a-z]+)?>\s*$/;
const TAGGED_BLOCK_BASE = "domios";

function isMultiline(text: string): boolean {
  return /\r?\n/.test(String(text || ""));
}

function pickTaggedBlockTag(text: string): string {
  let tag = TAGGED_BLOCK_BASE;
  let suffix = 1;
  while (text.includes(`{${tag}`) || text.includes(`{/${tag}}`)) {
    tag = `${TAGGED_BLOCK_BASE}${suffix}`;
    suffix += 1;
  }
  return tag;
}

function wrapMultiline(text: string): string {
  const body = String(text || "");
  const tag = pickTaggedBlockTag(body);
  return `{${tag}\n${body}\n${tag}}`;
}

function deliverText(text: string): string {
  return isMultiline(text) ? wrapMultiline(text) : String(text || "");
}

function pluginSettings(): Record<string, unknown> {
  const raw = typeof host.settingsJson === "function" ? host.settingsJson() : null;
  if (!raw) return {};
  try {
    const parsed = typeof raw === "string" ? JSON.parse(raw) : raw;
    return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

function withLaunchEnv(command: string, p: LaunchParams): string {
  const platform = host.platform();
  const quote = (v: string) => host.shell.quoteFor(v, platform);
  // On Windows Domios launches agents in PowerShell; the host reports the
  // platform as "windows".
  const isPowerShell = platform === "windows";
  const settings = pluginSettings();
  const exports: string[] = [];
  const nonce =
    (typeof p.sessionId === "string" && p.sessionId) ||
    (typeof p.launchMarker === "string" && p.launchMarker) ||
    null;
  if (nonce) {
    exports.push(
      isPowerShell
        ? `$env:CODETERM_SESSION_BINDING_NONCE=${quote(nonce)};`
        : `export CODETERM_SESSION_BINDING_NONCE=${quote(nonce)};`,
    );
  }
  const apiBase =
    (typeof settings.apiBase === "string" && settings.apiBase) ||
    (typeof p.apiBase === "string" && p.apiBase) ||
    null;
  if (apiBase) {
    exports.push(
      isPowerShell
        ? `$env:OPENAI_API_BASE=${quote(apiBase)};`
        : `export OPENAI_API_BASE=${quote(apiBase)};`,
    );
  }
  const apiKeySecret =
    (typeof settings.apiKeySecret === "string" && settings.apiKeySecret) ||
    (typeof p.apiKeySecret === "string" && p.apiKeySecret) ||
    null;
  if (apiKeySecret) {
    // The key is fetched at run time inside the shell so its value never
    // appears in the command text, argv, logs or scrollback.
    exports.push(
      isPowerShell
        ? `$env:OPENAI_API_KEY=(codeterm mem secret get --name ${quote(apiKeySecret)});`
        : `export OPENAI_API_KEY="$(codeterm mem secret get --name ${quote(apiKeySecret)})";`,
    );
  }
  if (exports.length === 0) return command;
  return `${exports.join(" ")} ${command}`;
}


function starterPrompt(params: LaunchParams): string | undefined {
  if (params.starterPromptText) return params.starterPromptText;
  if (params.starterPrompt === "no_starter") return undefined;
  if (params.starterPrompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (params.starterPrompt === "team_bootstrap") throw new Error("team_bootstrap requires host starterPromptText");
  return ASSIGNED_TASK_START_PROMPT;
}

/** Per-tab history folder: one file per launch, named by the binding nonce. */
function historyDir(cwd: string): string {
  return `${String(cwd || "")}/.aider/history`;
}

function historyFilePath(cwd: string, sessionId: string): string {
  return `${historyDir(cwd)}/${sessionId}.md`;
}

interface HistoryFileEntry {
  id: string;
  path: string;
  modifiedMs: number | null;
}

function historyFileEntries(cwd: string): HistoryFileEntry[] {
  const fs = (globalThis as { host?: { fs?: { readDir?: (p: string) => unknown[] } } }).host?.fs;
  if (!fs || typeof fs.readDir !== "function") return [];
  let entries: unknown[];
  try {
    entries = fs.readDir(historyDir(cwd)) as unknown[];
  } catch (_) {
    return [];
  }
  const out: HistoryFileEntry[] = [];
  for (const raw of entries) {
    const entry = raw as { name?: string; path?: string; modifiedMs?: number | null };
    if (!entry || typeof entry.name !== "string") continue;
    if (!entry.name.endsWith(".md")) continue;
    const id = entry.name.slice(0, -3);
    if (!id) continue;
    out.push({
      id,
      path: typeof entry.path === "string" && entry.path ? entry.path : historyFilePath(cwd, id),
      modifiedMs: typeof entry.modifiedMs === "number" ? entry.modifiedMs : null,
    });
  }
  return out;
}

function lastNonEmptyLine(text: string): string {
  const lines = String(text || "").split(/\r?\n/);
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (line) return line;
  }
  return "";
}

function extractBannerModel(text: string): string | null {
  const match = String(text || "").match(/Model:\s*([^\s]+)/);
  return match ? match[1] : null;
}

function extractPromptedCommands(screen: string): string[] {
  const lines = String(screen || "").split(/\r?\n/);
  const commands: string[] = [];
  for (let i = 0; i < lines.length; i++) {
    const match = lines[i].match(COMMAND_LINE_RE);
    if (match) commands.push(match[1].trim());
  }
  return commands;
}

function isCodetermOnlyCommandList(commands: string[]): boolean {
  return commands.length > 0 && commands.every((command) => command.startsWith("codeterm "));
}

function isStartupDialog(question: string): boolean {
  return GITIGNORE_QUESTION_RE.test(question) || DOCS_URL_QUESTION_RE.test(question);
}

function parsePrompt(text: string): ParsedPrompt | null {
  const screen = String(text || "");
  const lines = screen.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const question = lines[i].trim();
    if (!PROMPT_QUESTION_RE.test(question)) continue;
    if (isStartupDialog(question)) continue;
    const options: [string, string][] = [["Yes", "1"], ["No", "2"]];
    return { question, options };
  }
  return null;
}

function parsePromptWithContext(text: string): ParsedPrompt | null {
  const screen = String(text || "");
  const lines = screen.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const question = lines[i].trim();
    if (!PROMPT_QUESTION_RE.test(question)) continue;
    const options: [string, string][] = [["Yes", "1"], ["No", "2"]];
    return { question, options };
  }
  return null;
}

function promptSafeChoice(prompt: ParsedPrompt | null | undefined, screen: string): string | null {
  if (!prompt || !prompt.question) return null;
  if (GITIGNORE_QUESTION_RE.test(prompt.question)) return "1";
  if (DOCS_URL_QUESTION_RE.test(prompt.question)) return "3";
  if (!SHELL_COMMAND_QUESTION_RE.test(prompt.question)) return null;
  return isCodetermOnlyCommandList(extractPromptedCommands(screen)) ? "1" : null;
}

const plugin: PluginModule = {
  starterPromptText(prompt: "no_starter" | "assigned_task" | "idle_wakeup" | "team_bootstrap" | "startup_handshake"): string | undefined {
    return prompt === "team_bootstrap" ? undefined : starterPrompt({ starterPrompt: prompt });
  },

  detectFromTitle(title: string): boolean {
    return TITLE_RE.test(String(title || ""));
  },

  detectFromOutput(text: string): boolean {
    const t = String(text || "");
    let hasMarker = false;
    for (let i = 0; i < OUTPUT_MARKERS.length; i++) {
      if (t.indexOf(OUTPUT_MARKERS[i]) !== -1) { hasMarker = true; break; }
    }
    if (!hasMarker) return false;
    for (let j = 0; j < OUTPUT_SIGNALS.length; j++) {
      if (t.indexOf(OUTPUT_SIGNALS[j]) !== -1) return true;
    }
    return false;
  },

  screenHasTui(screen: string): boolean {
    const text = String(screen || "");
    const lower = text.toLowerCase();
    let hasFragment = false;
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) { hasFragment = true; break; }
    }
    const prompt = lastNonEmptyLine(text);
    if (!MODE_PROMPT_RE.test(prompt)) return false;
    // A mode-prefixed prompt (e.g. "ask>") is itself an aider signal.
    if (/^[a-z]+>$/.test(prompt)) return true;
    // A bare prompt with no aider marker is not an aider screen.
    return hasFragment && /aider/i.test(text);
  },

  isOutputNoise(text: string): boolean {
    const trimmed = String(text || "").trim();
    if (!trimmed) return true;
    for (let i = 0; i < trimmed.length; i++) {
      const c = trimmed[i];
      if (
        c !== "─" && c !== "━" && c !== "═" && c !== "█" && c !== "▀" &&
        c !== " " && c !== "\t" && c !== "\n" && c !== "\r"
      ) {
        return false;
      }
    }
    return true;
  },

  hasEventMarkers(chunk: string): boolean {
    return parsePrompt(chunk) !== null || parsePromptWithContext(chunk) !== null;
  },

  detectEvents(text: string): string[] {
    return (parsePrompt(text) || parsePromptWithContext(text)) ? ["permission_request"] : [];
  },

  discoverModels(): ModelInfo[] | null {
    return null;
  },

  modelGroupUsage(): [] {
    return [];
  },

  updatePromptSafeChoice(prompt: ParsedPrompt): string | null {
    return promptSafeChoice(prompt, prompt.question);
  },

  parsePrompt,


  buildPromptResponse(optionIndex: number, _screen?: string): string[] {
    const ENTER = "DQ==";
    const YES = "WQ==";
    const NO = "Tg==";
    const DONT_ASK = "RA==";
    if (optionIndex === 2) return [DONT_ASK, ENTER];
    return [optionIndex === 0 ? YES : NO, ENTER];
  },

  buildLaunchCommand(params: LaunchParams): string {
    const p = params || {};
    const platform = host.platform();
    const quote = (v: string) => host.shell.quoteFor(v, platform);
    const isPowerShell = platform === "windows";
    const cwd = typeof p.cwd === "string" && p.cwd ? p.cwd : ".";
    const historyDirPath = `${cwd}/.aider/history`;
    // The nonce must expand at run time, so the path is a double-quoted
    // string with the variable left unquoted; only the directory is escaped.
    const escapeDir = (value: string) =>
      isPowerShell
        ? value.replace(/`/g, "``").replace(/"/g, '`"')
        : value.replace(/(["\\$`])/g, "\\$1");
    const dirQuoted = escapeDir(historyDirPath);
    const chatHistoryPath = isPowerShell
      ? `"${dirQuoted}/$($env:CODETERM_SESSION_BINDING_NONCE).md"`
      : `"${dirQuoted}/\${CODETERM_SESSION_BINDING_NONCE}.md"`;
    const inputHistoryPath = isPowerShell
      ? `"${dirQuoted}/$($env:CODETERM_SESSION_BINDING_NONCE).input"`
      : `"${dirQuoted}/\${CODETERM_SESSION_BINDING_NONCE}.input"`;
    const parts: string[] = ["aider"];
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) {
        parts.push(quote(String(p.args[i])));
      }
    }
    parts.push("--no-auto-commits", "--no-pretty", "--no-fancy-input", "--no-show-model-warnings", "--chat-language", "English");
    const settings = pluginSettings();
    const configPath =
      (typeof settings.configPath === "string" && settings.configPath) ||
      (typeof p.configPath === "string" && p.configPath) ||
      null;
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
      inputHistoryPath,
    ].join(" ");
    const mkdir = isPowerShell
      ? `New-Item -ItemType Directory -Force -Path ${quote(historyDirPath)} | Out-Null;`
      : `mkdir -p ${quote(historyDirPath)};`;
    return withLaunchEnv(`${mkdir} ${command} ${historyFlags}`, p);
  },


  buildResumeCommand(_sessionId: string, _skipPermissions?: boolean): string {
    return this.buildLaunchCommand({});
  },

  buildResumeCommandWithContext(params: ResumeParams): string {
    return this.buildLaunchCommand({ ...params, task: params.systemPrompt || undefined });
  },

  buildPromptSafeChoice: promptSafeChoice,

  launchOnboardingResponse(screen: string): { frameKey: string; step: string } | null {
    const prompt = parsePromptWithContext(String(screen || ""));
    if (!prompt) return null;
    const choice = promptSafeChoice(prompt, String(screen || ""));
    if (!choice) return null;
    const keys = this.buildPromptResponse!(Number(choice) - 1, String(screen || ""));
    return { frameKey: prompt.question, step: keys[0] };
  },

  launchOnboardingSafeChoice(screen: string): string | null {
    const prompt = parsePromptWithContext(String(screen || ""));
    return prompt ? promptSafeChoice(prompt, String(screen || "")) : null;
  },

  chatPreprocess(ctx: { provider?: string | null; text: string }): { text: string } | null {
    if (ctx?.provider !== "aider") return null;
    const text = String(ctx?.text || "");
    const delivered = deliverText(text);
    return delivered === text ? null : { text: delivered };
  },

  detectSessionId(cwd: string, exclude?: string[] | string, maxAgeMs?: number): string | null {
    const excluded = Array.isArray(exclude) ? exclude : exclude ? [exclude] : [];
    const limit = typeof maxAgeMs === "number" && maxAgeMs > 0 ? maxAgeMs : 0;
    const now = Date.now();
    let newest: HistoryFileEntry | null = null;
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

  detectLaunchSession(evidence: {
    cwd: string;
    launchMarker?: string | null;
    launchedAtMs: number;
    processes?: unknown[];
  }): { sessionId: string; source: "pid_registry" | "launch_marker" } | null {
    const marker = evidence && typeof evidence.launchMarker === "string" ? evidence.launchMarker : "";
    if (!marker) return null;
    const cwd = evidence && typeof evidence.cwd === "string" ? evidence.cwd : "";
    if (!cwd) return null;
    return historyFileEntries(cwd).some((entry) => entry.id === marker)
      ? { sessionId: marker, source: "launch_marker" }
      : null;
  },

  enumerateSessions(): unknown[] {
    return [];
  },

  findSession(_sessionId: string): unknown | null {
    return null;
  },

  sessionExists(cwd: string, sessionId: string): boolean {
    if (!sessionId) return false;
    return historyFileEntries(cwd).some((entry) => entry.id === sessionId);
  },

  sessionCreatedMs(_cwd: string, _sessionId: string): number | null {
    return null;
  },

  detectActiveSession(_cwd: string, _currentSid?: string): string | null {
    return null;
  },

  detectSessionModel(_cwd: string, _sessionId: string): string | null {
    return null;
  },

  usageSources(_cwd: string, _sessionId: string): { path: string; sessionId?: string }[] {
    return [];
  },

  sessionFilePath(cwd: string, sessionId: string): string | null {
    if (!cwd || !sessionId) return null;
    return historyFilePath(cwd, sessionId);
  },

  sessionJsonlPath(cwd: string, sessionId: string): string | null {
    if (!cwd || !sessionId) return null;
    return historyFilePath(cwd, sessionId);
  },

  parseSessionDelta(chunk: string): unknown {
    return parseAiderHistoryDelta(String(chunk || ""));
  },

  checkIntegration(): unknown {
    return {
      config_file_exists: false,
      plugin_file_exists: false,
      plugin_file_owned: false,
      plugin_file_current: false,
      plugin_installed: false,
    };
  },

  installIntegration(_apiPort: number, _options: { consent: boolean }): unknown {
    return { ok: false, error: "aider has no integration to install" };
  },

  uninstallIntegration(): unknown {
    return { ok: true };
  },

  ensureIntegrationOnStartup(_apiPort: number): unknown {
    return this.checkIntegration!();
  },
};

export default plugin;
export {
  deliverText,
  extractBannerModel,
  lastNonEmptyLine,
  MODE_PROMPT_RE,
  pickTaggedBlockTag,
  promptSafeChoice,
  parsePromptWithContext,
  wrapMultiline,
};
