// OpenCode (sst/opencode) provider plugin (runtime logic, capability: agentProvider).
// ProviderPlugin contract is the separate src/index.ts artifact the app imports.
import type { LaunchParams, ModelInfo, ParsedPrompt, PluginModule } from "@codeterm/plugin-sdk";

const CODETERM_OPENCODE_PLUGIN_MARKER = "codeterm-opencode-session-binding-v2";
const CODETERM_OPENCODE_PLUGIN_FILE = "codeterm-session-binding.js";
const TITLE_RE = /opencode/i;
// Two-part fingerprint: opencode mention + TUI/prompt signal to avoid false positives.
const OUTPUT_MARKERS = ["opencode", "OpenCode", "OpenCode Zen"];
const OUTPUT_SIGNALS = [
  "Permission required",
  "Allow this tool call",
  "Approve this command",
  "█▀▀█",
  "Build · ",
  "┃",
  "╹",
];
const TUI_FRAGMENTS = ["tab agents", "ctrl+p commands"];
const ASSIGNED_TASK_START_PROMPT = "Execute the assigned task. Follow the configured instructions.";
const IDLE_WAKEUP_START_PROMPT = "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";

function starterPrompt(params: LaunchParams): string {
  if (params.starterPrompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (params.starterPrompt === "team_bootstrap") {
    if (!params.starterPromptText) throw new Error("team_bootstrap requires host starterPromptText");
    return params.starterPromptText;
  }
  return ASSIGNED_TASK_START_PROMPT;
}
const QUESTION_MENU_RE = /^\s*(?:[>❯▶➤→●]\s*)?(\d+)[.)]\s+(.+?)\s*$/;
const QUESTION_MENU_SELECTORS = [">", "❯", "▶", "➤", "→", "●"];
const PICKER_TITLE_RE = /^(Select model|Connect a provider)\b.*\besc\b/i;
let lastPromptShape: "horizontal" | "vertical" = "horizontal";

const plugin: PluginModule = {
  starterPromptText(prompt: "assigned_task" | "idle_wakeup" | "team_bootstrap"): string | undefined {
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
        c !== "─" && c !== "━" && c !== "═" && c !== "█" && c !== "▀" &&
        c !== " " && c !== "\t" && c !== "\n" && c !== "\r"
      ) {
        return false;
      }
    }
    return true;
  },

  hasEventMarkers(chunk: string): boolean {
    const c = String(chunk || "");
    const lc = c.toLowerCase();
    return (
      c.indexOf("Permission required") !== -1 ||
      lc.indexOf("approve") !== -1 ||
      chunkHasQuestionMenuRow(c) ||
      findModalPickerTitle(c) >= 0 ||
      (lc.indexOf("select") !== -1 && lc.indexOf("submit") !== -1 && lc.indexOf("dismiss") !== -1)
    );
  },

  detectEvents(text: string): string[] {
    const t = String(text || "");
    const events: string[] = [];
    if (
      t.indexOf("Permission required") !== -1 ||
      t.indexOf("Allow this tool call") !== -1 ||
      t.indexOf("Approve this command") !== -1 ||
      parseQuestionMenuPrompt(t) ||
      parseModalPickerPrompt(t)
    ) {
      events.push("permission_request");
    }
    return events;
  },

  // off hot reads. If the CLI is absent/offline, use the manifest seed.
  discoverModels(): ModelInfo[] {
    return discoverOpencodeModels() || manifestModels();
  },

  // because response bytes depend on horizontal vs vertical navigation.
  parsePrompt(text: string): ParsedPrompt | null {
    const screen = String(text || "");
    return parseQuestionMenuPrompt(screen) || parseModalPickerPrompt(screen) || parsePermissionPrompt(screen);
  },

  // opencode has both horizontal permission rows and vertical question menus.
  buildPromptResponse(optionIndex: number, screen?: string): string[] {
    const DOWN = "G1tC";  // ESC [ B
    const UP = "G1tB";    // ESC [ A
    const RIGHT = "G1tD"; // ESC [ C
    const ENTER = "DQ==";  // \r
    const steps: string[] = [];
    const target = optionIndex > 0 ? optionIndex : 0;
    const promptShape = screen && isVerticalPrompt(String(screen)) ? "vertical" : lastPromptShape;
    if (promptShape === "vertical") {
      const current = screen ? verticalPromptHighlightIndex(String(screen)) : -1;
      const start = current >= 0 ? current : 0;
      const delta = target - start;
      const move = delta >= 0 ? DOWN : UP;
      for (let i = 0; i < Math.abs(delta); i++) steps.push(move);
    } else {
      for (let i = 0; i < target; i++) steps.push(RIGHT);
    }
    steps.push(ENTER);
    return steps;
  },

  buildLaunchCommand(params: LaunchParams): string {
    const p = params || {};
    const parts: string[] = [];
    let localTaskPath: string | null = null;
    if (p.task) {
      const taskPath = host.fs.writeTempFile(p.task, ".ct-task.md");
      if (!taskPath) throw new Error("failed to write OpenCode task instructions");
      const platform = host.platform();
      const taskName = taskPath.split(/[\\/]/).pop()!.replace(/[^A-Za-z0-9._-]/g, "-");
      localTaskPath = `.codeterm/${taskName}`;
      const source = host.shell.quoteFor(taskPath, platform);
      const local = host.shell.quoteFor(localTaskPath, platform);
      const config = host.shell.quoteFor(JSON.stringify({ instructions: [localTaskPath] }), platform);
      parts.push(platform === "windows"
        ? `New-Item -ItemType Directory -Force -Path '.codeterm' | Out-Null; Copy-Item -LiteralPath ${source} -Destination ${local} -Force; $env:OPENCODE_CONFIG_CONTENT = ${config}; try {`
        : `mkdir -p .codeterm && cp -- ${source} ${local} && OPENCODE_CONFIG_CONTENT=${config}`);
    }
    parts.push("opencode");
    // reply over HTTP instead of scraping the PTY picker.
    const sps = p.structuredPromptServer;
    if (sps && sps.port) {
      const platform = host.platform();
      parts.push("--hostname", host.shell.quoteFor(String(sps.hostname || "127.0.0.1"), platform));
      parts.push("--port", host.shell.quoteFor(String(sps.port), platform));
    }
    if (p.sessionId) {
      parts.push("--session", host.shell.quoteFor(String(p.sessionId), host.platform()));
    }
    if (p.args && p.args.length > 0) {
      // Quote each arg — caller-supplied via the spawn API; prevents shell injection.
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    if (p.task) {
      parts.push("--prompt", host.shell.quoteFor(starterPrompt(p), host.platform()));
    }
    if (localTaskPath) {
      const local = host.shell.quoteFor(localTaskPath, host.platform());
      parts.push(host.platform() === "windows"
        ? `} finally { Remove-Item -LiteralPath ${local} -Force -ErrorAction SilentlyContinue }`
        : `; ct_status=$?; rm -f -- ${local}; exit $ct_status`);
    }
    return parts.join(" ");
  },

  buildResumeCommand(sessionId: string, skipPermissions?: boolean): string {
    // Resume uses the normal TUI path; permission rules come from saved session config.
    void skipPermissions;
    const parts = ["opencode", "--session", host.shell.quoteFor(sessionId, host.platform())];
    return parts.join(" ");
  },

  // absent from opencode's env, or event field structure changed in a new release).
  // agent_session_id.is_some() short-circuits the Rust call-site before this runs.
  detectSessionId(_cwd: string, exclude?: string[] | string, maxAgeMs?: number): string | null {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    const windowMs = maxAgeMs != null && maxAgeMs > 0 ? maxAgeMs : 300000;
    const excludeList: string[] = Array.isArray(exclude)
      ? (exclude as string[])
      : (typeof exclude === "string" && exclude ? [exclude] : []);
    const rows = host.fs.sqliteQuery<{ id: string }>(
      db,
      "SELECT id FROM session WHERE time_created > (strftime('%s','now') * 1000 - ?1) ORDER BY time_created DESC LIMIT 5",
      [windowMs]
    );
    for (let i = 0; i < rows.length; i++) {
      const id = rows[i].id;
      if (typeof id !== "string" || !id.startsWith("ses_")) continue;
      if (excludeList.indexOf(id) !== -1) continue;
      return id;
    }
    return null;
  },

  enumerateSessions(): unknown[] {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return [];
    const rows = host.fs.sqliteQuery<{
      id?: string;
      directory?: string;
      title?: string;
      time_created?: number;
      time_updated?: number;
    }>(db, "SELECT id, directory, title, time_created, time_updated FROM session ORDER BY time_updated DESC", []);
    return rows.filter((row) => typeof row.id === "string" && row.id.startsWith("ses_")).map((row) => ({
      sessionId: row.id,
      provider: "opencode",
      projectPath: typeof row.directory === "string" && row.directory ? row.directory : null,
      title: typeof row.title === "string" && row.title ? row.title : null,
      summary: null,
      filePath: db,
      createdTs: typeof row.time_created === "number" ? Math.floor(row.time_created / 1000) : null,
      modifiedTs: typeof row.time_updated === "number" ? Math.floor(row.time_updated / 1000) : null,
    }));
  },

  checkIntegration(): unknown {
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const pluginExists = !!pluginPath && host.fs.fileExists(pluginPath);
    const pluginBody = pluginExists ? String(host.fs.readFile(pluginPath!) || "") : "";
    // Require the binding-nonce header too; a pre-nonce plugin file is outdated → reinstall.
    const pluginCurrent = pluginExists &&
      pluginBody.indexOf(CODETERM_OPENCODE_PLUGIN_MARKER) !== -1 &&
      pluginBody.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1;
    if (!cfg || !host.fs.fileExists(cfg)) {
      return {
        config_file_exists: false,
        plugin_file_exists: pluginExists,
        plugin_file_current: pluginCurrent,
        plugin_installed: false,
      };
    }
    const json = host.fs.readJson<OpencodeConfig>(cfg);
    if (!json) {
      return {
        config_file_exists: true,
        plugin_file_exists: pluginExists,
        plugin_file_current: pluginCurrent,
        plugin_installed: false,
      };
    }
    const target = opencodePluginEntry();
    const entries = json.plugin;
    const installed = Array.isArray(entries) && entries.some((entry) => {
      return opencodePluginEntryMatches(entry, target);
    });
    return {
      config_file_exists: true,
      plugin_file_exists: pluginExists,
      plugin_file_current: pluginCurrent,
      plugin_installed: installed,
    };
  },

  installIntegration(apiPort: number): unknown {
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const target = opencodePluginEntry();
    if (!cfg || !pluginPath || !target) return { ok: false, error: "could not resolve opencode config path" };

    const json = (host.fs.fileExists(cfg)
      ? (host.fs.readJson<OpencodeConfig>(cfg) || { "$schema": "https://opencode.ai/config.json" })
      : { "$schema": "https://opencode.ai/config.json" }) as OpencodeConfig;
    if (!json || typeof json !== "object" || Array.isArray(json)) {
      return { ok: false, error: "opencode.json root must be an object" };
    }
    if (!Array.isArray(json.plugin)) json.plugin = [];
    json.plugin = json.plugin.filter((entry) => !opencodePluginEntryMatches(entry, target));
    json.plugin.push(target);

    if (!host.fs.writeFile(pluginPath, opencodeBindingPluginSource(apiPort | 0))) {
      return { ok: false, error: "writeFile failed for " + pluginPath };
    }
    if (!host.fs.writeJson(cfg, json)) {
      return { ok: false, error: "writeJson failed for " + cfg };
    }
    host.log("info", "[opencode-plugin] session binding plugin installed at " + pluginPath);
    return { ok: true };
  },

  uninstallIntegration(): unknown {
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const target = opencodePluginEntry();
    if (cfg && host.fs.fileExists(cfg)) {
      const json = host.fs.readJson<OpencodeConfig>(cfg);
      if (json && typeof json === "object" && Array.isArray(json.plugin)) {
        json.plugin = json.plugin.filter((entry) => !opencodePluginEntryMatches(entry, target));
        if (!host.fs.writeJson(cfg, json)) {
          return { ok: false, error: "writeJson failed for " + cfg };
        }
      }
    }
    if (pluginPath && host.fs.fileExists(pluginPath) && !host.fs.removeFile(pluginPath)) {
      return { ok: false, error: "removeFile failed for " + pluginPath };
    }
    return { ok: true };
  },

  ensureIntegrationOnStartup(apiPort: number): unknown {
    const status = this.checkIntegration!() as {
      plugin_installed?: boolean;
      plugin_file_exists?: boolean;
      plugin_file_current?: boolean;
    };
    if (status && status.plugin_installed && status.plugin_file_exists && status.plugin_file_current) return { ok: true };
    return this.installIntegration!(apiPort);
  },

  sessionExists(_cwd: string, sessionId: string): boolean {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return false;
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT 1 AS one FROM session WHERE id = ?1 LIMIT 1",
      [sessionId]
    );
    return rows.length > 0;
  },

  // birth-time guard to sqlite-backed sessions, which have no per-session file.
  sessionCreatedMs(_cwd: string, sessionId: string): number | null {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    const rows = host.fs.sqliteQuery<{ time_created?: number }>(
      db,
      "SELECT time_created FROM session WHERE id = ?1 LIMIT 1",
      [sessionId]
    );
    if (!rows.length) return null;
    const tc = rows[0].time_created;
    return typeof tc === "number" ? tc : null;
  },

  detectActiveSession(_cwd: string, _currentSid?: string): string | null {
    return null;
  },

  // the model recorded against this session_id. No "latest model in cwd"
  // fallback — that would hand the tab a SIBLING agent's model. Unknown → null.
  detectSessionModel(_cwd: string, sessionId: string): string | null {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    if (!sessionId) return null;
    return scanMessagesForModel(db,
      "SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created DESC LIMIT 20",
      [sessionId]);
  },

  // part.time_created seen, passed back to fetch only newer parts next poll.
  usesStructuredChat(): boolean { return true; },

  readStructuredChat(_cwd: string, sessionId: string, cursor?: string | null): unknown {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return { messages: [], cursor: cursor || null };
    const sinceMs = cursor ? (parseInt(cursor, 10) || 0) : 0;
    const rows = host.fs.sqliteQuery<{ id: string | number; ts?: number; part?: string; msg?: string }>(
      db,
      "SELECT p.id AS id, p.time_created AS ts, p.data AS part, m.data AS msg " +
        "FROM part p JOIN message m ON m.id = p.message_id " +
        "WHERE p.session_id = ?1 AND p.time_created >= ?2 " +
        "ORDER BY p.time_created ASC, p.id ASC",
      [sessionId, sinceMs]
    );
    const messages: unknown[] = [];
    let maxTs = sinceMs;
    for (let i = 0; i < rows.length; i++) {
      const ts = rows[i].ts || 0;
      if (ts > maxTs) maxTs = ts;
      const part = safeJson(rows[i].part);
      if (!part) continue;
      let role = "assistant";
      const msg = safeJson(rows[i].msg);
      if (msg && (msg as { role?: string }).role) role = (msg as { role: string }).role;
      const cm = partToChatMsg(part as OpencodePart, role, String(rows[i].id), ts);
      if (cm) messages.push(cm);
    }
    return { messages: messages, cursor: String(maxTs) };
  },
};

// --- types shared by the helpers above ---

interface OpencodeManifest { models?: ModelInfo[] }
interface OpencodeConfig { "$schema"?: string; plugin?: unknown[] }
interface OpencodePart {
  type?: string;
  text?: string;
  tool?: string;
  state?: { input?: unknown; output?: unknown };
}

function parsePermissionPrompt(text: string): ParsedPrompt | null {
  const lines = String(text || "").split("\n").map(cleanPromptLine);
  let permIdx = -1;
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].indexOf("Permission required") !== -1) { permIdx = i; break; }
  }
  if (permIdx < 0) return null;

  let optLine: string | null = null;
  for (let j = permIdx; j < lines.length; j++) {
    const lc = lines[j].toLowerCase();
    if (
      (lc.indexOf("allow") !== -1 || lc.indexOf("reject") !== -1 || lc.indexOf("deny") !== -1) &&
      lc.indexOf("permission required") === -1
    ) { optLine = lines[j]; break; }
  }
  if (!optLine) return null;
  const labels = optLine.split(/\s{2,}/).map((s) => s.trim()).filter(Boolean);
  if (labels.length < 2) return null;

  let question = "";
  for (let k = permIdx + 1; k < lines.length; k++) {
    const q = lines[k];
    if (q && q.toLowerCase() !== "patterns") { question = q; break; }
  }
  lastPromptShape = "horizontal";
  return {
    question: question || "Permission required",
    options: labels.map((label, idx) => [label, String(idx + 1)]),
  };
}

interface QuestionMenuCluster {
  question: string;
  cluster: { value: string; label: string; idx: number; sel: boolean }[];
}

interface ModalPickerCluster {
  question: string;
  cluster: { value: string; label: string; idx: number; sel: boolean }[];
}

function collectQuestionMenuPrompt(text: string): QuestionMenuCluster | null {
  const lines = String(text || "").split("\n").map(cleanPromptLine);
  const start = Math.max(0, lines.length - 30);
  const rows: { value: string; label: string; idx: number; sel: boolean }[] = [];
  for (let i = start; i < lines.length; i++) {
    const line = lines[i];
    const m = QUESTION_MENU_RE.exec(line);
    if (m && m[2].trim()) {
      rows.push({ value: m[1], label: m[2].trim(), idx: i, sel: isQuestionMenuSelected(line) });
    }
  }
  if (rows.length < 2) return null;

  const cluster: typeof rows = [];
  let prev = rows[rows.length - 1].idx;
  for (let i = rows.length - 1; i >= 0; i--) {
    if (prev - rows[i].idx <= 2 || cluster.length === 0) {
      cluster.unshift(rows[i]);
      prev = rows[i].idx;
    } else break;
  }
  if (cluster.length < 2 || !hasQuestionMenuFooter(lines, cluster[cluster.length - 1].idx)) return null;

  let question = "";
  for (let i = cluster[0].idx - 1; i >= start && cluster[0].idx - i <= 6; i--) {
    const q = lines[i];
    if (q && q.charAt(0) !== "#" && !isQuestionMenuFooter(q)) { question = q; break; }
  }
  return { question: question || "Select an option", cluster: cluster };
}

function parseQuestionMenuPrompt(text: string): ParsedPrompt | null {
  const c = collectQuestionMenuPrompt(text);
  if (!c) return null;
  lastPromptShape = "vertical";
  return {
    question: c.question,
    options: c.cluster.map((row) => [row.label, row.value]),
  };
}

function collectModalPickerPrompt(text: string): ModalPickerCluster | null {
  const rawLines = String(text || "").split("\n");
  const titleIdx = findModalPickerTitle(text);
  if (titleIdx < 0) return null;

  const searchIdx = findModalSearchLine(rawLines, titleIdx);
  if (searchIdx < 0) return null;
  const modalCol = rawLines[searchIdx].indexOf("Search");
  if (modalCol < 0) return null;

  const question = cleanPromptLine(rawLines[titleIdx].slice(modalCol)).replace(/\s+esc$/i, "").trim();
  const rows: { value: string; label: string; idx: number; sel: boolean }[] = [];
  let nextValue = 1;
  for (let i = searchIdx + 1; i < Math.min(rawLines.length, searchIdx + 22); i++) {
    const raw = rawLines[i];
    const label = modalPickerLabel(raw, modalCol);
    if (!label) continue;
    if (isModalPickerSection(label)) continue;
    rows.push({
      value: String(nextValue++),
      label,
      idx: i,
      sel: raw.slice(Math.max(0, modalCol - 4), modalCol + 2).indexOf("●") !== -1,
    });
  }
  if (rows.length < 2) return null;
  return { question: question || "Select an option", cluster: rows };
}

function parseModalPickerPrompt(text: string): ParsedPrompt | null {
  const c = collectModalPickerPrompt(text);
  if (!c) return null;
  lastPromptShape = "vertical";
  return {
    question: c.question,
    options: c.cluster.map((row) => [row.label, row.value]),
  };
}

function cleanPromptLine(line: string): string {
  return String(line)
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .replace(/[│┃|╎╏┆┊╭╮╯╰╹▀]/g, " ")
    .replace(/^[\s←⚠»·]+/, "")
    .replace(/[\s·»]+$/, "")
    .trim();
}

function chunkHasQuestionMenuRow(chunk: string): boolean {
  const lines = String(chunk || "").split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (QUESTION_MENU_RE.test(cleanPromptLine(lines[i]))) return true;
  }
  return false;
}

function questionMenuHighlightIndex(text: string): number {
  const c = collectQuestionMenuPrompt(text);
  if (!c) return -1;
  return c.cluster.findIndex((row) => row.sel);
}

function verticalPromptHighlightIndex(text: string): number {
  const modal = collectModalPickerPrompt(text);
  if (modal) return modal.cluster.findIndex((row) => row.sel);
  return questionMenuHighlightIndex(text);
}

function isVerticalPrompt(text: string): boolean {
  return !!collectQuestionMenuPrompt(text) || !!collectModalPickerPrompt(text);
}

function isQuestionMenuSelected(line: string): boolean {
  const first = String(line || "").trim().charAt(0);
  return QUESTION_MENU_SELECTORS.indexOf(first) !== -1;
}

function findModalPickerTitle(text: string): number {
  const lines = String(text || "").split("\n");
  const start = Math.max(0, lines.length - 40);
  for (let i = lines.length - 1; i >= start; i--) {
    if (PICKER_TITLE_RE.test(cleanPromptLine(lines[i]))) return i;
  }
  return -1;
}

function findModalSearchLine(lines: string[], titleIdx: number): number {
  for (let i = titleIdx + 1; i < Math.min(lines.length, titleIdx + 8); i++) {
    if (cleanPromptLine(lines[i]) === "Search") return i;
  }
  return -1;
}

function modalPickerLabel(rawLine: string, modalCol: number): string {
  const cropped = rawLine.slice(modalCol, modalCol + 54);
  const label = cleanPromptLine(cropped).replace(/\s+/g, " ").trim();
  if (!label) return "";
  if (/^\/.+\b\d+\.\d+\.\d+\b/.test(label)) return "";
  return label;
}

function isModalPickerSection(label: string): boolean {
  const lc = label.toLowerCase();
  return (
    lc === "search" ||
    lc === "popular" ||
    lc === "providers" ||
    lc === "popular providers" ||
    lc.indexOf("view all providers") === 0
  );
}

function hasQuestionMenuFooter(lines: string[], lastOptionIdx: number): boolean {
  for (let i = lastOptionIdx + 1; i < Math.min(lines.length, lastOptionIdx + 7); i++) {
    if (isQuestionMenuFooter(lines[i])) return true;
  }
  return false;
}

function isQuestionMenuFooter(line: string): boolean {
  const lc = String(line || "").toLowerCase();
  return lc.indexOf("select") !== -1 && lc.indexOf("submit") !== -1 && lc.indexOf("dismiss") !== -1;
}

function discoverOpencodeModels(): ModelInfo[] | null {
  const raw = host.exec(JSON.stringify({
    bin: "opencode",
    args: ["models"],
    timeoutMs: 3000,
  }));
  const result = safeJson(raw) as { code?: number; stdout?: string } | null;
  if (!result || result.code !== 0 || typeof result.stdout !== "string") return null;
  const lines = result.stdout.split("\n");
  const models: ModelInfo[] = [];
  for (let i = 0; i < lines.length; i++) {
    const id = lines[i].trim();
    if (!isModelId(id)) continue;
    if (isNonChatModel(id)) continue;
    models.push({ id: id, displayName: displayNameFromId(id) });
  }
  // list is already availability-filtered — every entry is selectable (available unset).
  return models.length ? models : null;
}

function isModelId(value: string): boolean {
  return /^[a-z0-9][a-z0-9_.-]*\/[a-z0-9][a-z0-9_.-]*$/i.test(value);
}

// embedding/tts/moderation/rerank ids so only chat models reach the picker.
const NON_CHAT_TOKENS = ["embed", "embedding", "embeddings", "tts", "whisper", "moderation", "rerank", "dall", "dalle"];
function isNonChatModel(id: string): boolean {
  const leaf = (id.split("/").pop() || id).toLowerCase();
  const toks = leaf.split(/[-_.]/);
  for (let i = 0; i < toks.length; i++) if (NON_CHAT_TOKENS.indexOf(toks[i]) !== -1) return true;
  return false;
}

function manifestModels(): ModelInfo[] {
  try {
    const m = JSON.parse(host.manifestJson() || "null") as OpencodeManifest | null;
    return Array.isArray(m && m.models) ? m!.models! : [];
  } catch (_) {
    return [];
  }
}

function safeJson(s: string | undefined): unknown {
  try { return JSON.parse(String(s || "null")); } catch (_) { return null; }
}

function displayNameFromId(id: string): string {
  const slash = id.split("/");
  const provider = slash.length > 1 ? slash[0] : "";
  const leaf = slash[slash.length - 1] || id;
  const words = prettyLeaf(leaf);
  if (provider.toLowerCase() === "opencode") return "OpenCode Zen " + words;
  return words;
}

// number onto the prior numeric token as a dotted version (4-8 -> 4.8).
function prettyLeaf(leaf: string): string {
  const parts = leaf.split("-").filter(Boolean);
  const out: string[] = [];
  for (let i = 0; i < parts.length; i++) {
    const p = parts[i];
    if (/^\d+$/.test(p) && out.length && /\d$/.test(out[out.length - 1])) {
      out[out.length - 1] = out[out.length - 1] + "." + p;
    } else {
      out.push(capitalizeToken(p));
    }
  }
  return out.join(" ");
}

function capitalizeToken(token: string): string {
  if (!token) return token;
  if (token === "gpt") return "GPT";
  if (token === "ai") return "AI";
  return token.charAt(0).toUpperCase() + token.substring(1);
}

// serde shape). text→user/assistant, reasoning→thinking, tool→tool_call.
function partToChatMsg(part: OpencodePart, role: string, id: string, tsMs: number): unknown {
  const timestamp = String(tsMs);
  const type = part && part.type;
  if (type === "text") {
    const txt = (part.text || "");
    if (!txt.trim()) return null;
    return { id: id, type: role === "user" ? "user" : "assistant", content: txt, timestamp: timestamp };
  }
  if (type === "reasoning") {
    const rtxt = (part.text || "");
    if (!rtxt.trim()) return null;
    return { id: id, type: "thinking", content: rtxt, timestamp: timestamp };
  }
  if (type === "tool") {
    const tool = part.tool || "tool";
    const state = part.state || {};
    const canonical = canonicalToolFields(tool, state.input);
    const out: Record<string, unknown> = {
      id: id, type: "tool_call", content: "", timestamp: timestamp, toolName: tool,
      toolKind: canonical.kind,
      toolSummary: canonical.summary,
      toolDetail: canonical.detail,
      toolEdits: canonical.edits,
    };
    if (state.input !== undefined) out.toolInput = state.input;
    if (typeof state.output === "string") out.toolResult = state.output;
    return out;
  }
  return null;
}

interface CanonicalToolFields {
  kind: string;
  summary: string;
  detail: string;
  edits: Array<{ path: string; old: string; new: string }>;
}

function canonicalToolFields(name: string, input: unknown): CanonicalToolFields {
  const tool = String(name || "").trim();
  const value = input && typeof input === "object" && !Array.isArray(input)
    ? input as Record<string, unknown>
    : {};
  const text = (key: string): string => typeof value[key] === "string" ? value[key] as string : "";
  const command = text("command") || text("cmd");
  const path = text("file_path") || text("filePath") || text("path");
  const pattern = text("pattern");
  const edits = extractCanonicalEdits(value);
  const lower = tool.toLowerCase();
  if (lower === "bash" || lower === "shell" || lower === "exec" || lower === "run") {
    return { kind: "command", summary: cleanToolText(command, "Command"), detail: command, edits: [] };
  }
  if (lower === "edit" || lower === "multiedit" || lower === "write") {
    return { kind: "edit", summary: path || "File edit", detail: path, edits: edits };
  }
  if (lower === "read") {
    return { kind: "read", summary: path || "Read file", detail: path, edits: [] };
  }
  if (lower === "glob" || lower === "grep" || lower === "search") {
    return { kind: "search", summary: cleanToolText(pattern || path, "Search"), detail: pattern || path, edits: [] };
  }
  return { kind: "generic", summary: cleanToolText(tool, "Tool"), detail: tool, edits: edits };
}

function extractCanonicalEdits(input: Record<string, unknown>): Array<{ path: string; old: string; new: string }> {
  const parentPath = typeof input.file_path === "string" ? input.file_path : typeof input.filePath === "string" ? input.filePath : typeof input.path === "string" ? input.path : "";
  const pair = (value: unknown, fallbackPath: string): { path: string; old: string; new: string } | null => {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const item = value as Record<string, unknown>;
    const oldValue = typeof item.old_string === "string" ? item.old_string : item.oldString;
    const newValue = typeof item.new_string === "string" ? item.new_string : item.newString;
    if (typeof oldValue !== "string" && typeof newValue !== "string") return null;
    const path = typeof item.file_path === "string" ? item.file_path : typeof item.filePath === "string" ? item.filePath : typeof item.path === "string" ? item.path : fallbackPath;
    return { path: path, old: typeof oldValue === "string" ? oldValue : "", new: typeof newValue === "string" ? newValue : "" };
  };
  if (Array.isArray(input.edits)) return input.edits.map((edit) => pair(edit, parentPath)).filter((e): e is { path: string; old: string; new: string } => e !== null);
  const single = pair(input, parentPath);
  return single ? [single] : [];
}

function cleanToolText(value: string, fallback: string): string {
  const text = String(value || "").replace(/[\r\n\t]+/g, " ").trim();
  if (!text) return fallback;
  return text.length > 120 ? text.slice(0, 117) + "..." : text;
}

function opencodeDbPath(): string | null {
  let base = host.envGet("XDG_DATA_HOME");
  if (!base) base = host.expandHome("~/.local/share");
  if (!base) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return base + sep + "opencode" + sep + "opencode.db";
}

function opencodeConfigDir(): string | null {
  const override = host.envGet("OPENCODE_CONFIG_DIR");
  if (override) return override;
  return host.expandHome("~/.config/opencode");
}

function opencodeConfigPath(): string | null {
  const dir = opencodeConfigDir();
  if (!dir) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return dir + sep + "opencode.json";
}

function opencodeBindingPluginPath(): string | null {
  const dir = opencodeConfigDir();
  if (!dir) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return dir + sep + CODETERM_OPENCODE_PLUGIN_FILE;
}

function opencodePluginEntry(): string | null {
  const path = opencodeBindingPluginPath();
  return path ? path.replace(/\\/g, "/") : null;
}

function opencodePluginEntryMatches(entry: unknown, target: string | null): boolean {
  if (!target) return false;
  let value: string | null = null;
  if (typeof entry === "string") value = entry;
  else if (Array.isArray(entry) && typeof entry[0] === "string") value = entry[0];
  if (!value) return false;
  return value.replace(/\\/g, "/") === target;
}

function opencodeBindingPluginSource(apiPort: number): string {
  return [
    "// " + CODETERM_OPENCODE_PLUGIN_MARKER,
    "const PROVIDER_ID = \"opencode\";",
    "const FALLBACK_PORT = \"" + String(apiPort || 7686) + "\";",
    "",
    "function endpoint() {",
    "  const tab = process.env.CODETERM_TAB_ID;",
    "  if (!tab) return null;",
    "  const base =",
    "    process.env.CODETERM_API_URL ||",
    "    `http://127.0.0.1:${process.env.CODETERM_API_PORT || FALLBACK_PORT}`;",
    "  return `${base}/api/tabs/${encodeURIComponent(tab)}/hook/session_start`;",
    "}",
    "",
    "function normalizeSessionID(value) {",
    "  return typeof value === \"string\" && value.startsWith(\"ses_\") ? value : null;",
    "}",
    "",
    "function firstSessionID(...values) {",
    "  for (const value of values) {",
    "    const sessionID = normalizeSessionID(value);",
    "    if (sessionID) return sessionID;",
    "  }",
    "  return null;",
    "}",
    "",
    "async function report(sessionID, source, directory) {",
    "  sessionID = normalizeSessionID(sessionID);",
    "  if (!sessionID) return;",
    "  const url = endpoint();",
    "  if (!url) return;",
    "  const headers = {",
    "    \"content-type\": \"application/json\",",
    "    \"x-codeterm-agent-provider\": PROVIDER_ID,",
    "  };",
    "  if (process.env.CODETERM_SESSION_BINDING_NONCE) {",
    "    headers[\"x-codeterm-binding-nonce\"] = process.env.CODETERM_SESSION_BINDING_NONCE;",
    "  }",
    "  if (process.env.CODETERM_API_TOKEN) {",
    "    headers.authorization = `Bearer ${process.env.CODETERM_API_TOKEN}`;",
    "  }",
    "  await fetch(url, {",
    "    method: \"POST\",",
    "    headers,",
    "    body: JSON.stringify({",
    "      provider_id: PROVIDER_ID,",
    "      session_id: sessionID,",
    "      source,",
    "      cwd: directory || process.cwd(),",
    "    }),",
    "  }).catch(() => {});",
    "}",
    "",
    "function eventSessionID(event) {",
    "  const p = event?.properties || {};",
    "  return firstSessionID(",
    "    p.sessionID,",
    "    p.session_id,",
    "    p.session?.id,",
    "    p.info?.sessionID,",
    "    p.info?.session_id,",
    "    p.message?.sessionID,",
    "    p.message?.session_id,",
    "    p.part?.sessionID,",
    "    p.part?.session_id,",
    "    event?.sessionID,",
    "    event?.session_id,",
    "    p.info?.id",
    "  );",
    "}",
    "",
    "function eventDirectory(event, fallback) {",
    "  return (",
    "    event?.properties?.info?.directory ||",
    "    event?.properties?.session?.directory ||",
    "    event?.properties?.directory ||",
    "    fallback",
    "  );",
    "}",
    "",
    "export default async function CodeTermSessionBindingPlugin({ directory }) {",
    "  return {",
    "    async event({ event }) {",
    "      if (",
    "        event?.type === \"session.created\" ||",
    "        event?.type === \"session.updated\" ||",
    "        event?.type === \"tui.session.select\" ||",
    "        event?.type === \"message.updated\" ||",
    "        event?.type === \"message.part.updated\"",
    "      ) {",
    "        await report(eventSessionID(event), event.type, eventDirectory(event, directory));",
    "      }",
    "    },",
    "    async \"chat.message\"(input) {",
    "      await report(input?.sessionID, \"chat.message\", directory);",
    "    },",
    "    async \"tool.execute.before\"(input) {",
    "      await report(input?.sessionID, \"tool.execute.before\", directory);",
    "    },",
    "    async \"shell.env\"(input, output) {",
    "      await report(input?.sessionID, \"shell.env\", input?.cwd || directory);",
    "      return output;",
    "    },",
    "  };",
    "}",
    "",
  ].join("\n");
}

function scanMessagesForModel(db: string, sql: string, params: unknown[]): string | null {
  const rows = host.fs.sqliteQuery<{ data?: string }>(db, sql, params);
  for (let i = 0; i < rows.length; i++) {
    const raw = rows[i].data;
    if (!raw) continue;
    try {
      const v = JSON.parse(raw) as { role?: string; modelID?: string };
      if (v && v.role === "assistant" && typeof v.modelID === "string" && v.modelID) {
        return v.modelID;
      }
    } catch (_) {
    }
  }
  return null;
}

export default plugin;
