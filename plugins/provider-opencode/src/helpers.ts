import type { Host, ModelInfo, ParsedPrompt } from "@codeterm/plugin-sdk";

// Constants and regexes
export const CODETERM_OPENCODE_PLUGIN_MARKER = "codeterm-opencode-session-binding-v3";
export const CODETERM_OPENCODE_PLUGIN_OWNERSHIP_PREFIX = "codeterm-opencode-session-binding-v";
export const QUESTION_MENU_RE = /^\s*(?:[>❯▶➤→●]\s*)?(\d+)[.)]\s+(.+?)\s*$/;
export const QUESTION_MENU_SELECTORS = [">", "❯", "▶", "➤", "→", "●"];
export const PICKER_TITLE_RE = /^(Select model|Connect a provider)\b.*\besc\b/i;
// json_extract parses a row whole; only user rows carrying diffs outgrow these bounds.
export const ASSISTANT_MESSAGE_MAX_BYTES = 1 << 20;
export const MESSAGE_ROLE_MAX_BYTES = 16 << 20;
export let lastPromptShape: "horizontal" | "vertical" = "horizontal";

// Types
export interface QuestionMenuCluster {
  question: string;
  cluster: { value: string; label: string; idx: number; sel: boolean }[];
}

export interface ModalPickerCluster {
  question: string;
  cluster: { value: string; label: string; idx: number; sel: boolean }[];
}

export interface ModelGroupUsage {
  group: string;
  messages: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  cost_micros: number;
  spend_cents?: number;
  spend_limit_cents?: number;
  spend_remaining_cents?: number;
  spend_resets_at_ms?: number;
}

export interface OpencodeConfig {
  $schema?: string;
  plugin?: unknown[];
}
export interface OpencodeSessionRow {
  id?: string;
  directory?: string;
  title?: string;
  time_created?: number;
  time_updated?: number;
}
export interface OpencodePart {
  type?: string;
  text?: string;
  tool?: string;
  state?: { input?: unknown; output?: unknown };
}

export interface ModelUsageRow {
  g?: string;
  messages?: number;
  input?: number;
  output?: number;
  cache_read?: number;
  cache_write?: number;
  cost?: number;
}

export interface CanonicalToolFields {
  kind: string;
  summary: string;
  detail: string;
  edits: Array<{ path: string; old: string; new: string }>;
}

// Prompt parsing
export function parsePermissionPrompt(text: string): ParsedPrompt | null {
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

export function collectQuestionMenuPrompt(text: string): QuestionMenuCluster | null {
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

export function parseQuestionMenuPrompt(text: string): ParsedPrompt | null {
  const c = collectQuestionMenuPrompt(text);
  if (!c) return null;
  lastPromptShape = "vertical";
  return {
    question: c.question,
    options: c.cluster.map((row) => [row.label, row.value]),
  };
}

export function collectModalPickerPrompt(text: string): ModalPickerCluster | null {
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

export function parseModalPickerPrompt(text: string): ParsedPrompt | null {
  const c = collectModalPickerPrompt(text);
  if (!c) return null;
  lastPromptShape = "vertical";
  return {
    question: c.question,
    options: c.cluster.map((row) => [row.label, row.value]),
  };
}

// Prompt line and element analysis
export function cleanPromptLine(line: string): string {
  return String(line)
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .replace(/[│┃|╎╏┆┊╭╮╯╰╹▀]/g, " ")
    .replace(/^[\s←⚠»·]+/, "")
    .replace(/[\s·»]+$/, "")
    .trim();
}

export function chunkHasQuestionMenuRow(chunk: string): boolean {
  const lines = String(chunk || "").split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (QUESTION_MENU_RE.test(cleanPromptLine(lines[i]))) return true;
  }
  return false;
}

export function questionMenuHighlightIndex(text: string): number {
  const c = collectQuestionMenuPrompt(text);
  if (!c) return -1;
  return c.cluster.findIndex((row) => row.sel);
}

export function verticalPromptHighlightIndex(text: string): number {
  const modal = collectModalPickerPrompt(text);
  if (modal) return modal.cluster.findIndex((row) => row.sel);
  return questionMenuHighlightIndex(text);
}

export function isVerticalPrompt(text: string): boolean {
  return !!collectQuestionMenuPrompt(text) || !!collectModalPickerPrompt(text);
}

export function isQuestionMenuSelected(line: string): boolean {
  const first = String(line || "").trim().charAt(0);
  return QUESTION_MENU_SELECTORS.indexOf(first) !== -1;
}

export function findModalPickerTitle(text: string): number {
  const lines = String(text || "").split("\n");
  const start = Math.max(0, lines.length - 40);
  for (let i = lines.length - 1; i >= start; i--) {
    if (PICKER_TITLE_RE.test(cleanPromptLine(lines[i]))) return i;
  }
  return -1;
}

export function findModalSearchLine(lines: string[], titleIdx: number): number {
  for (let i = titleIdx + 1; i < Math.min(lines.length, titleIdx + 8); i++) {
    if (cleanPromptLine(lines[i]) === "Search") return i;
  }
  return -1;
}

export function modalPickerLabel(rawLine: string, modalCol: number): string {
  const cropped = rawLine.slice(modalCol, modalCol + 54);
  const label = cleanPromptLine(cropped).replace(/\s+/g, " ").trim();
  if (!label) return "";
  if (/^\/.+\b\d+\.\d+\.\d+\b/.test(label)) return "";
  return label;
}

export function isModalPickerSection(label: string): boolean {
  const lc = label.toLowerCase();
  return (
    lc === "search" ||
    lc === "popular" ||
    lc === "providers" ||
    lc === "popular providers" ||
    lc.indexOf("view all providers") === 0
  );
}

export function hasQuestionMenuFooter(lines: string[], lastOptionIdx: number): boolean {
  for (let i = lastOptionIdx + 1; i < Math.min(lines.length, lastOptionIdx + 7); i++) {
    if (isQuestionMenuFooter(lines[i])) return true;
  }
  return false;
}

export function isQuestionMenuFooter(line: string): boolean {
  const lc = String(line || "").toLowerCase();
  return lc.indexOf("select") !== -1 && lc.indexOf("submit") !== -1 && lc.indexOf("dismiss") !== -1;
}

// Model discovery (requires host.exec, host.fs, etc. passed as parameter or from global host)
declare const host: Host;

export function discoverOpencodeModels(): ModelInfo[] | null {
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
    models.push({ id: id, displayName: displayNameFromId(id), group: groupFromModelId(id) });
  }
  return models.length ? models : null;
}

export function groupFromModelId(id: string): string | undefined {
  const slash = id.split("/");
  if (slash.length < 2) return undefined;
  const provider = slash[0].trim();
  if (!provider) return undefined;
  if (provider.toLowerCase() === "opencode") return "OpenCode Zen";
  return provider;
}

export function isModelId(value: string): boolean {
  return /^[a-z0-9][a-z0-9_.-]*\/[a-z0-9][a-z0-9_.-]*$/i.test(value);
}

const NON_CHAT_TOKENS = ["embed", "embedding", "embeddings", "tts", "whisper", "moderation", "rerank", "dall", "dalle", "image", "video", "imagine"];
export function isNonChatModel(id: string): boolean {
  const leaf = (id.split("/").pop() || id).toLowerCase();
  const toks = leaf.split(/[-_.]/);
  for (let i = 0; i < toks.length; i++) if (NON_CHAT_TOKENS.indexOf(toks[i]) !== -1) return true;
  return false;
}

// Group usage and balance
export function discoverOpencodeGroupUsage(): ModelGroupUsage[] {
  const db = opencodeDbPath();
  if (!db || !host.fs.fileExists(db)) return [];
  const rows = host.fs.sqliteQuery<ModelUsageRow>(
    db,
    "SELECT json_extract(data,'$.providerID') AS g, " +
      "COUNT(*) AS messages, " +
      "SUM(COALESCE(json_extract(data,'$.tokens.input'),0)) AS input, " +
      "SUM(COALESCE(json_extract(data,'$.tokens.output'),0)) AS output, " +
      "SUM(COALESCE(json_extract(data,'$.tokens.cache.read'),0)) AS cache_read, " +
      "SUM(COALESCE(json_extract(data,'$.tokens.cache.write'),0)) AS cache_write, " +
      "SUM(COALESCE(json_extract(data,'$.cost'),0)) AS cost " +
      "FROM message WHERE octet_length(data) <= " + ASSISTANT_MESSAGE_MAX_BYTES + " " +
      "AND json_extract(data,'$.role')='assistant' " +
      "AND json_extract(data,'$.providerID') IS NOT NULL GROUP BY g",
    []
  );
  const out: ModelGroupUsage[] = [];
  for (let i = 0; i < rows.length; i++) {
    const group = rows[i].g;
    if (typeof group !== "string" || !group) continue;
    const costMicros = Math.round(finiteNumber(rows[i].cost) * 1_000_000);
    const entry: ModelGroupUsage = {
      group: group,
      messages: finiteNumber(rows[i].messages),
      input_tokens: finiteNumber(rows[i].input),
      output_tokens: finiteNumber(rows[i].output),
      cache_read_tokens: finiteNumber(rows[i].cache_read),
      cache_write_tokens: finiteNumber(rows[i].cache_write),
      cost_micros: costMicros,
    };
    if (costMicros > 0) entry.spend_cents = Math.round(costMicros / 10_000);
    const remaining = groupBalanceCents(group);
    if (remaining != null) entry.spend_remaining_cents = remaining;
    out.push(entry);
  }
  return out;
}

const BALANCE_BY_GROUP: Record<string, { url: string; secret: string }> = {
  deepseek: { url: "https://api.deepseek.com/user/balance", secret: "deepseekKey" },
};
const BALANCE_CREDENTIAL = "opencodeAuth";
const BALANCE_TTL_MS = 60_000;
const BALANCE_TIMEOUT_MS = 4000;
let balanceCache: { group: string; atMs: number; cents: number | null } | null = null;

export function groupBalanceCents(group: string): number | null {
  if (!BALANCE_BY_GROUP[group]) return null;
  const now = host.unixNowMs();
  if (balanceCache && balanceCache.group === group && now - balanceCache.atMs < BALANCE_TTL_MS) {
    return balanceCache.cents;
  }
  const cents = fetchBalanceCents(group);
  balanceCache = { group: group, atMs: now, cents: cents };
  return cents;
}

export function fetchBalanceCents(group: string): number | null {
  const spec = BALANCE_BY_GROUP[group];
  if (!spec || typeof host.fetch !== "function") return null;
  let response: { status?: number; body?: string } | null = null;
  try {
    response = safeJson(host.fetch(JSON.stringify({
      url: spec.url,
      method: "GET",
      timeoutMs: BALANCE_TIMEOUT_MS,
      credential: {
        id: BALANCE_CREDENTIAL,
        headers: { Authorization: "Bearer {" + spec.secret + "}" },
      },
    }))) as { status?: number; body?: string } | null;
  } catch (_) {
    return null;
  }
  if (!response || response.status !== 200 || typeof response.body !== "string") return null;
  const body = safeJson(response.body) as
    | { balance_infos?: Array<{ currency?: string; total_balance?: string }> }
    | null;
  const infos = body && Array.isArray(body.balance_infos) ? body.balance_infos : [];
  let chosen: { currency?: string; total_balance?: string } | null = null;
  for (let i = 0; i < infos.length; i++) {
    if (infos[i] && infos[i].currency === "USD") { chosen = infos[i]; break; }
  }
  if (!chosen && infos.length > 0) chosen = infos[0];
  if (!chosen || typeof chosen.total_balance !== "string") return null;
  const total = parseFloat(chosen.total_balance);
  return Number.isFinite(total) ? Math.round(total * 100) : null;
}

// Utility functions
export function finiteNumber(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

export function safeJson(s: string | undefined): unknown {
  try { return JSON.parse(String(s || "null")); } catch (_) { return null; }
}

export function normalizeSessionDirectory(value: unknown): string {
  let normalized = String(value || "");
  if (host.platform() === "windows") {
    normalized = normalized.replace(/\\/g, "/");
    while (normalized.length > 1 && normalized.endsWith("/") && !/^[a-z]:\/$/i.test(normalized)) {
      normalized = normalized.slice(0, -1);
    }
    return normalized.toLowerCase();
  }
  while (normalized.length > 1 && normalized.endsWith("/")) normalized = normalized.slice(0, -1);
  return normalized;
}

export function parseStructuredCursor(cursor?: string | null): [number, string[]] | null {
  if (cursor == null) return [0, []];
  const parsed = safeJson(cursor || "");
  if (!Array.isArray(parsed) || parsed.length !== 2) return null;
  if (typeof parsed[0] !== "number" || !Number.isSafeInteger(parsed[0]) || parsed[0] < 0) return null;
  if (!Array.isArray(parsed[1]) || !parsed[1].every((id) => typeof id === "string" && id.length > 0)) return null;
  const ts = parsed[0];
  const ids = parsed[1] as string[];
  return [ts, ids];
}

// Display names
export function displayNameFromId(id: string): string {
  const slash = id.split("/");
  const provider = slash.length > 1 ? slash[0] : "";
  const leaf = slash[slash.length - 1] || id;
  const words = prettyLeaf(leaf);
  if (provider.toLowerCase() === "opencode") return "OpenCode Zen " + words;
  return words;
}

export function prettyLeaf(leaf: string): string {
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

export function capitalizeToken(token: string): string {
  if (!token) return token;
  if (token === "gpt") return "GPT";
  if (token === "ai") return "AI";
  return token.charAt(0).toUpperCase() + token.substring(1);
}

// Message and tool conversion
export function partToChatMsg(part: OpencodePart, role: string, id: string, tsMs: number): unknown {
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

export function canonicalToolFields(name: string, input: unknown): CanonicalToolFields {
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

export function extractCanonicalEdits(input: Record<string, unknown>): Array<{ path: string; old: string; new: string }> {
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

export function cleanToolText(value: string, fallback: string): string {
  const text = String(value || "").replace(/[\r\n\t]+/g, " ").trim();
  if (!text) return fallback;
  return text.length > 120 ? text.slice(0, 117) + "..." : text;
}

// Config and path helpers
export function opencodeDbPath(): string | null {
  const windows = host.platform() === "windows";
  const sep = windows ? "\\" : "/";
  const configuredDb = host.envGet("OPENCODE_DB");
  if (configuredDb === ":memory:") return null;
  if (configuredDb) {
    const absolute = windows
      ? /^[A-Za-z]:[\\/]|^[\\/]{1,2}/.test(configuredDb)
      : configuredDb.startsWith("/");
    if (absolute) return configuredDb;
  }
  const base = host.envGet("XDG_DATA_HOME") || host.expandHome("~/.local/share");
  if (!base) return null;
  const dataDir = base + sep + "opencode";
  return configuredDb ? dataDir + sep + configuredDb : dataDir + sep + "opencode.db";
}

export function opencodeSessionRecord(row: OpencodeSessionRow, db: string): unknown {
  return {
    sessionId: row.id,
    provider: "opencode",
    projectPath: typeof row.directory === "string" && row.directory ? row.directory : null,
    title: typeof row.title === "string" && row.title ? row.title : null,
    summary: null,
    filePath: db,
    createdTs: typeof row.time_created === "number" ? Math.floor(row.time_created / 1000) : null,
    modifiedTs: typeof row.time_updated === "number" ? Math.floor(row.time_updated / 1000) : null,
  };
}

export function opencodeConfigDir(): string | null {
  const override = host.envGet("OPENCODE_CONFIG_DIR");
  if (override) return override;
  return host.expandHome("~/.config/opencode");
}

export function opencodeConfigPath(): string | null {
  const dir = opencodeConfigDir();
  if (!dir) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return dir + sep + "opencode.json";
}

const CODETERM_OPENCODE_PLUGIN_FILE = "codeterm-session-binding.js";

export function opencodeBindingPluginPath(): string | null {
  const dir = opencodeConfigDir();
  if (!dir) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return dir + sep + CODETERM_OPENCODE_PLUGIN_FILE;
}

export function opencodePluginEntry(): string | null {
  const path = opencodeBindingPluginPath();
  return path ? path.replace(/\\/g, "/") : null;
}

export function opencodePluginEntryMatches(entry: unknown, target: string | null): boolean {
  if (!target) return false;
  let value: string | null = null;
  if (typeof entry === "string") value = entry;
  else if (Array.isArray(entry) && typeof entry[0] === "string") value = entry[0];
  if (!value) return false;
  return value.replace(/\\/g, "/") === target;
}

export function isCurrentOpencodeBindingPluginSource(text: string): boolean {
  const currentHeader = "// " + CODETERM_OPENCODE_PLUGIN_MARKER;
  const hasCurrentHeader = text.startsWith(currentHeader + "\n")
    || text.startsWith(currentHeader + "\r\n");
  const required = [
    "CODETERM_API_URL",
    "CODETERM_API_TOKEN",
    "CODETERM_SESSION_BINDING_NONCE",
    "CODETERM_TAB_ID",
    "/api/tabs/",
    "x-codeterm-binding-nonce",
    "authorization",
    "sessionID === lastReported",
  ];
  return hasCurrentHeader
    && required.every((value) => text.includes(value))
    && !/CODETERM_API_PORT|FALLBACK_PORT|127\.0\.0\.1|localhost|:\d{2,5}/i.test(text);
}

export function ownsOpencodeBindingPluginSource(text: string): boolean {
  const header = "// " + CODETERM_OPENCODE_PLUGIN_OWNERSHIP_PREFIX;
  return text.startsWith(header) && /^\d+(?:\r?\n|$)/.test(text.slice(header.length));
}

export function opencodeBindingPluginSource(): string {
  return [
    "// " + CODETERM_OPENCODE_PLUGIN_MARKER,
    "const PROVIDER_ID = \"opencode\";",
    "let lastReported = null;",
    "",
    "function endpoint() {",
    "  const tab = process.env.CODETERM_TAB_ID;",
    "  const token = process.env.CODETERM_API_TOKEN;",
    "  const nonce = process.env.CODETERM_SESSION_BINDING_NONCE;",
    "  if (!tab) return null;",
    "  if (!token || !nonce || !process.env.CODETERM_API_URL) return null;",
    "  const base = process.env.CODETERM_API_URL.replace(/\\/+$/, \"\");",
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
    "  if (!sessionID || sessionID === lastReported) return;",
    "  const url = endpoint();",
    "  if (!url) return;",
    "  lastReported = sessionID;",
    "  const headers = {",
    "    \"content-type\": \"application/json\",",
    "    \"x-codeterm-agent-provider\": PROVIDER_ID,",
    "    \"x-codeterm-binding-nonce\": process.env.CODETERM_SESSION_BINDING_NONCE,",
    "    authorization: `Bearer ${process.env.CODETERM_API_TOKEN}`,",
    "  };",
    "  const delivered = await fetch(url, {",
    "    method: \"POST\",",
    "    headers,",
    "    body: JSON.stringify({",
    "      provider_id: PROVIDER_ID,",
    "      session_id: sessionID,",
    "      source,",
    "      cwd: directory || process.cwd(),",
    "    }),",
    "  }).then((response) => response.ok, () => false);",
    "  if (!delivered && lastReported === sessionID) lastReported = null;",
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

export function scanMessagesForModel(db: string, sql: string, params: unknown[]): string | null {
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

export function scanMessagesForQualifiedModel(db: string, sessionId: string): string | null {
  try {
    const rows = host.fs.sqliteQuery<{ data?: string }>(
      db,
      "SELECT data FROM message WHERE session_id = ?1 AND octet_length(data) <= ?2 " +
        "ORDER BY time_created DESC LIMIT 20",
      [sessionId, ASSISTANT_MESSAGE_MAX_BYTES],
    );
    for (let i = 0; i < rows.length; i++) {
      const raw = rows[i].data;
      if (!raw) continue;
      try {
        const value = JSON.parse(raw) as { role?: string; providerID?: string; modelID?: string };
        if (
          value.role === "assistant" &&
          typeof value.providerID === "string" && value.providerID &&
          typeof value.modelID === "string" && value.modelID
        ) {
          return `${value.providerID}/${value.modelID}`;
        }
      } catch (_) {
      }
    }
  } catch (_) {
  }
  return null;
}

export function scanSessionDirectory(db: string, sessionId: string): string | null {
  try {
    const rows = host.fs.sqliteQuery<{ id?: string; directory?: string }>(
      db,
      "SELECT id, directory FROM session WHERE id = ?1 LIMIT 1",
      [sessionId],
    );
    const session = rows.find((row) => row.id === sessionId);
    return typeof session?.directory === "string" && session.directory.trim()
      ? session.directory
      : null;
  } catch (_) {
    return null;
  }
}
