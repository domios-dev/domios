import type {
  LaunchParams,
  ModelInfo,
  ParsedPrompt,
  PluginModule,
  ResumeParams,
  SettingField,
} from "@codeterm/plugin-sdk";
import { parseAiderHistory, readAiderHistory } from "./history";

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
const MULTILINE_BLOCK_RE = /\{multiline\}([\s\S]*?)\{\/multiline\}/g;
const MULTILINE_BLOCK_BODY_RE = /\{multiline\}([\s\S]*?)\{\/multiline\}/;
const MULTILINE_BLOCK_START = "{multiline}";
const MULTILINE_BLOCK_END = "{/multiline}";
const MULTILINE_BLOCK_MAX_CHARS = 4000;

function isMultiline(text: string): boolean {
  return /\r?\n/.test(String(text || ""));
}

function wrapMultilineBlock(body: string): string {
  return `${MULTILINE_BLOCK_START}\n${body}\n${MULTILINE_BLOCK_END}`;
}

function splitMultilineBlocks(text: string): string[] {
  const source = String(text || "");
  const blocks: string[] = [];
  let last = 0;
  MULTILINE_BLOCK_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = MULTILINE_BLOCK_RE.exec(source)) !== null) {
    if (match.index > last) blocks.push(source.slice(last, match.index));
    blocks.push(match[0]);
    last = match.index + match[0].length;
  }
  if (last < source.length) blocks.push(source.slice(last));
  return blocks;
}

function wrapMultilineSegment(segment: string): string {
  const body = segment.replace(/^\n+/, "").replace(/\n+$/, "");
  if (!body) return segment;
  if (body.length <= MULTILINE_BLOCK_MAX_CHARS) return wrapMultilineBlock(body);
  const lines = body.split(/\r?\n/);
  const chunks: string[] = [];
  let current: string[] = [];
  let currentLen = 0;
  for (const line of lines) {
    const lineLen = line.length + 1;
    if (current.length > 0 && currentLen + lineLen > MULTILINE_BLOCK_MAX_CHARS) {
      chunks.push(wrapMultilineBlock(current.join("\n")));
      current = [];
      currentLen = 0;
    }
    current.push(line);
    currentLen += lineLen;
  }
  if (current.length > 0) chunks.push(wrapMultilineBlock(current.join("\n")));
  return chunks.join("\n");
}

function wrapMultiline(text: string): string {
  return splitMultilineBlocks(text)
    .map((segment) =>
      MULTILINE_BLOCK_BODY_RE.test(segment) ? segment : wrapMultilineSegment(segment),
    )
    .join("");
}

function deliverText(text: string): string {
  return isMultiline(text) ? wrapMultiline(text) : String(text || "");
}

function withLaunchEnv(command: string, p: LaunchParams): string {
  const platform = host.platform();
  const quote = (v: string) => host.shell.quoteFor(v, platform);
  // On Windows Domios launches agents in PowerShell; the host reports the
  // platform as "windows".
  const isPowerShell = platform === "windows";
  const exports: string[] = [];
  const apiBase = typeof p.apiBase === "string" && p.apiBase ? p.apiBase : null;
  if (apiBase) {
    exports.push(
      isPowerShell
        ? `$env:OPENAI_API_BASE=${quote(apiBase)};`
        : `export OPENAI_API_BASE=${quote(apiBase)};`,
    );
  }
  const apiKeySecret = typeof p.apiKeySecret === "string" && p.apiKeySecret ? p.apiKeySecret : null;
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

function parsePrompt(text: string): ParsedPrompt | null {
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

  parsePrompt: parsePromptWithContext,

  settingsSchema(): SettingField[] {
    return [
      { kind: "text", id: "configPath", label: "Aider config file", help: "Path to an aider YAML config file; passed as --config." },
      { kind: "text", id: "apiBase", label: "API base URL", help: "Base URL for the OpenAI-compatible API; exported as OPENAI_API_BASE." },
      { kind: "text", id: "apiKeySecret", label: "API key secret name", help: "Name of a Domios secret holding the API key; its value is injected only into the launch environment as OPENAI_API_KEY." },
    ];
  },

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
    const parts: string[] = ["aider"];
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) {
        parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
      }
    }
    parts.push("--no-auto-commits", "--no-pretty", "--no-fancy-input", "--no-show-model-warnings", "--chat-language", "English");
    const configPath = typeof p.configPath === "string" && p.configPath ? p.configPath : null;
    if (configPath) {
      parts.push("--config", host.shell.quoteFor(configPath, host.platform()));
    }
    const prompt = starterPrompt(p);
    if (p.task && prompt) {
      parts.push("--message", host.shell.quoteFor(prompt, host.platform()));
    }
    return withLaunchEnv(parts.join(" "), p);
  },

  buildResumeCommand(_sessionId: string, _skipPermissions?: boolean): string {
    return this.buildLaunchCommand({});
  },

  buildResumeCommandWithContext(params: ResumeParams): string {
    return this.buildLaunchCommand({ ...params, task: params.systemPrompt || undefined });
  },

  buildPromptSafeChoice: promptSafeChoice,

  chatPreprocess(ctx: { text: string }): { text: string } | null {
    const text = String(ctx?.text || "");
    const delivered = deliverText(text);
    return delivered === text ? null : { text: delivered };
  },

  detectSessionId(_cwd: string, _exclude?: string[] | string, _maxAgeMs?: number): string | null {
    return null;
  },

  enumerateSessions(): unknown[] {
    return [];
  },

  findSession(_sessionId: string): unknown | null {
    return null;
  },

  sessionExists(_cwd: string, _sessionId: string): boolean {
    return false;
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

  usesStructuredChat(): boolean {
    return true;
  },

  readStructuredChat(cwd: string, _sessionId: string, cursor?: string | null): unknown {
    const historyPath = `${cwd}/.aider.chat.history.md`;
    const result = readAiderHistory(historyPath, 0, cursor);
    if (!result) return { messages: [], cursor: cursor || null };
    return {
      messages: result.messages.map((m, i) => ({
        id: `hist_${i}`,
        type: m.role,
        content: m.text,
        seq: i,
      })),
      cursor: result.cursor,
    };
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
  promptSafeChoice,
  parsePromptWithContext,
  wrapMultiline,
};
