// OpenCode (sst/opencode) provider plugin (runtime logic, capability: agentProvider).
// ProviderPlugin contract is the separate src/index.ts artifact the app imports.
import type { LaunchParams, ModelInfo, ParsedPrompt, PluginModule, ResumeParams } from "@codeterm/plugin-sdk";
import {
  lastPromptShape,
  parsePermissionPrompt,
  parseQuestionMenuPrompt,
  parseModalPickerPrompt,
  chunkHasQuestionMenuRow,
  findModalPickerTitle,
  isVerticalPrompt,
  verticalPromptHighlightIndex,
  discoverOpencodeModels,
  discoverOpencodeGroupUsage,
  normalizeSessionDirectory,
  parseStructuredCursor,
  opencodeDbPath,
  opencodeConfigPath,
  opencodeBindingPluginPath,
  opencodePluginEntry,
  opencodePluginEntryMatches,
  isCurrentOpencodeBindingPluginSource,
  ownsOpencodeBindingPluginSource,
  opencodeBindingPluginSource,
  opencodeSessionRecord,
  partToChatMsg,
  scanMessagesForModel,
  scanMessagesForQualifiedModel,
  scanSessionDirectory,
  ASSISTANT_MESSAGE_MAX_BYTES,
  MESSAGE_ROLE_MAX_BYTES,
  safeJson,
  OpencodePart,
  OpencodeConfig,
  OpencodeSessionRow,
  ModelGroupUsage,
} from "./helpers";
import { readOpencodeSessionContent, SessionContentQuery } from "./sessionContent";

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

function existingBindingPluginIsUnowned(pluginPath: string): boolean {
  if (!host.fs.fileExists(pluginPath)) return false;
  return !ownsOpencodeBindingPluginSource(String(host.fs.readFile(pluginPath) || ""));
}

function starterPrompt(params: LaunchParams): string | undefined {
  if (params.starterPromptText) return params.starterPromptText;
  if (params.starterPrompt === "no_starter") return undefined;
  if (params.starterPrompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (params.starterPrompt === "team_bootstrap") throw new Error("team_bootstrap requires host starterPromptText");
  return ASSIGNED_TASK_START_PROMPT;
}

function withPadPermissionConfig(command: string, instructions?: string[], cleanup?: string): string {
  const platform = host.platform();
  const config = {
    permission: { external_directory: { "*": "ask" } },
    ...(instructions ? { instructions } : {}),
  };
  if (platform === "windows") {
    const encoded = host.shell.quoteFor(JSON.stringify(config), platform);
    return `$ct_config = ${encoded} | ConvertFrom-Json; $ct_pad_native = ([string]$env:CODETERM_PAD_DIR).TrimEnd([char[]]@([char]92, [char]47)); $ct_pad = $ct_pad_native -replace '\\\\', '/'; if ($ct_pad_native) { $ct_config.permission.external_directory | Add-Member -NotePropertyName ($ct_pad_native + '\\*') -NotePropertyValue 'allow' -Force; $ct_config.permission.external_directory | Add-Member -NotePropertyName ($ct_pad + '/*') -NotePropertyValue 'allow' -Force }; $env:OPENCODE_CONFIG_CONTENT = $ct_config | ConvertTo-Json -Depth 10 -Compress; try { ${command} } finally { Remove-Item Env:OPENCODE_CONFIG_CONTENT -ErrorAction SilentlyContinue; ${cleanup || ""} }`;
  }
  const permissionJson = JSON.stringify({ permission: config.permission });
  const prefix = permissionJson.slice(0, -3);
  const suffix = `}}${instructions ? `,"instructions":${JSON.stringify(instructions)}` : ""}}`;
  const q = (value: string) => host.shell.quoteFor(value, platform);
  const finish = cleanup
    ? `; ct_status=$?; unset OPENCODE_CONFIG_CONTENT; ${cleanup}; exit $ct_status`
    : `; unset OPENCODE_CONFIG_CONTENT`;
  return `ct_pad_dir=$(printf '%s' "${"${CODETERM_PAD_DIR:-}"}" | tr '\\\\' '/' | sed 's/"/\\\\\\"/g'); OPENCODE_CONFIG_CONTENT=${q(prefix)}; if [ -n "$ct_pad_dir" ]; then OPENCODE_CONFIG_CONTENT="$OPENCODE_CONFIG_CONTENT,\\\"$ct_pad_dir/*\\\":\\\"allow\\\""; fi; OPENCODE_CONFIG_CONTENT="$OPENCODE_CONFIG_CONTENT"$(printf '%s' ${q(suffix)}); export OPENCODE_CONFIG_CONTENT; ${command}${finish}`;
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

  // off hot reads. `null` when the CLI cannot answer: the host keeps its seed.
  discoverModels(): ModelInfo[] | null {
    return discoverOpencodeModels();
  },

  // Cumulative usage per upstream company comes from OpenCode's session store.
  // Model-id prefixes match picker groups, keeping vendor knowledge out of core.
  modelGroupUsage(): ModelGroupUsage[] {
    return discoverOpencodeGroupUsage();
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
    const promptShape = screen && isVerticalPrompt(String(screen)) ? "vertical" : (lastPromptShape as "horizontal" | "vertical");
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
    let taskPath: string | null = null;
    if (p.task) {
      taskPath = host.fs.writeTempFile(p.task, ".ct-task.md");
      if (!taskPath) throw new Error("failed to write OpenCode task instructions");
    }
    parts.push("opencode");
    if (p.skipPermissions) parts.push("--dangerously-skip-permissions");
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
    const prompt = starterPrompt(p);
    if (p.task && prompt) {
      parts.push("--prompt", host.shell.quoteFor(prompt, host.platform()));
    }
    return withPadPermissionConfig(parts.join(" "), taskPath ? [taskPath] : undefined);
  },

  buildResumeCommand(sessionId: string, skipPermissions?: boolean): string {
    return withPadPermissionConfig(resumeLaunch(sessionId, skipPermissions).parts.join(" "));
  },

  // A resumed session re-reads `instructions`, so the composed prompt is
  // re-applied through the same session-config channel buildLaunchCommand uses.
  buildResumeCommandWithContext(params: ResumeParams): string {
    const p = params || ({} as ResumeParams);
    const platform = host.platform();
    const launch = resumeLaunch(p.sessionId, p.skipPermissions);
    const parts = launch.parts;
    const args = p.args ? resumeLaunchArgs(p.args, p.sessionId) : [];
    for (let i = 0; i < args.length; i++) {
      const arg = String(args[i]);
      if (arg === "-m" || arg === "--model") {
        parts.push(arg, host.shell.quoteFor(String(args[i + 1] || ""), platform));
        i++;
      } else {
        parts.push(arg.startsWith("-") ? arg : host.shell.quoteFor(arg, platform));
      }
    }
    const systemPrompt = p.systemPrompt;
    if (!systemPrompt || !systemPrompt.trim()) return withPadPermissionConfig(parts.join(" "));
    const taskPath = host.fs.writeTempFile(systemPrompt, ".ct-task.md");
    if (!taskPath) return withPadPermissionConfig(parts.join(" "));
    const project = launch.directory
      ? host.shell.quoteFor(launch.directory, platform)
      : null;
    const prologue = project
      ? platform === "windows" ? `Set-Location -LiteralPath ${project} -ErrorAction Stop;` : `cd -- ${project} &&`
      : "";
    return withPadPermissionConfig(`${prologue} ${parts.join(" ")}`, [taskPath]);
  },

  // absent from opencode's env, or event field structure changed in a new release).
  // agent_session_id.is_some() short-circuits the Rust call-site before this runs.
  detectSessionId(cwd: string, exclude?: string[] | string, maxAgeMs?: number): string | null {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    const targetDirectory = normalizeSessionDirectory(cwd);
    if (!targetDirectory) return null;
    const windowMs = maxAgeMs != null && maxAgeMs > 0 ? maxAgeMs : 300000;
    const excludeList: string[] = Array.isArray(exclude)
      ? (exclude as unknown[]).filter((id): id is string => typeof id === "string")
      : (typeof exclude === "string" && exclude ? [exclude] : []);
    const directoryAliases = targetDirectory.endsWith("/")
      ? [targetDirectory, targetDirectory]
      : [targetDirectory, targetDirectory + "/"];
    const directoryExpression = host.platform() === "windows"
      ? "lower(replace(directory, char(92), '/'))"
      : "directory";
    const excludeSql = excludeList.length
      ? " AND id NOT IN (" + excludeList.map((_, index) => "?" + String(index + 4)).join(", ") + ")"
      : "";
    const rows = host.fs.sqliteQuery<{ id: string; directory?: string }>(
      db,
      "SELECT id, directory FROM session WHERE time_created > (strftime('%s','now') * 1000 - ?1) " +
        "AND " + directoryExpression + " IN (?2, ?3)" + excludeSql +
        " ORDER BY time_created DESC LIMIT 20",
      [windowMs, ...directoryAliases, ...excludeList]
    );
    for (let i = 0; i < rows.length; i++) {
      const id = rows[i].id;
      if (typeof id !== "string" || !id.startsWith("ses_")) continue;
      if (normalizeSessionDirectory(rows[i].directory) !== targetDirectory) continue;
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
    return rows.filter((row) => typeof row.id === "string" && row.id.startsWith("ses_")).map((row) => opencodeSessionRecord(row, db));
  },

  findSession(sessionId: string): unknown | null {
    if (typeof sessionId !== "string" || !sessionId.startsWith("ses_")) return null;
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    const rows = host.fs.sqliteQuery<OpencodeSessionRow>(db, "SELECT id, directory, title, time_created, time_updated FROM session WHERE id = ?1 LIMIT 1", [sessionId]);
    return rows.length > 0 && rows[0].id === sessionId ? opencodeSessionRecord(rows[0], db) : null;
  },

  readSessionContent(sessionId: string, query?: SessionContentQuery | null): unknown {
    if (typeof sessionId !== "string" || !sessionId.startsWith("ses_")) return null;
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    return readOpencodeSessionContent(db, sessionId, query);
  },

  checkIntegration(): unknown {
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const pluginExists = !!pluginPath && host.fs.fileExists(pluginPath);
    const pluginBody = pluginExists ? String(host.fs.readFile(pluginPath!) || "") : "";
    const pluginOwned = pluginExists && ownsOpencodeBindingPluginSource(pluginBody);
    const pluginCurrent = pluginExists && isCurrentOpencodeBindingPluginSource(pluginBody);
    if (!cfg || !host.fs.fileExists(cfg)) {
      return {
        config_file_exists: false,
        plugin_file_exists: pluginExists,
        plugin_file_owned: pluginOwned,
        plugin_file_current: pluginCurrent,
        plugin_installed: false,
      };
    }
    const json = host.fs.readJson<OpencodeConfig>(cfg);
    if (!json) {
      return {
        config_file_exists: true,
        plugin_file_exists: pluginExists,
        plugin_file_owned: pluginOwned,
        plugin_file_current: pluginCurrent,
        plugin_installed: false,
        integration_state: "unreadable",
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
      plugin_file_owned: pluginOwned,
      plugin_file_current: pluginCurrent,
      plugin_installed: installed,
    };
  },

  installIntegration(
    _apiPort: number,
    options: { consent: boolean; precompact_memory_export?: boolean },
  ): unknown {
    if (!options || options.consent !== true) {
      return { ok: false, error: "provider integration installation requires explicit local-user consent" };
    }
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const target = opencodePluginEntry();
    if (!cfg || !pluginPath || !target) return { ok: false, error: "could not resolve opencode config path" };
    if (existingBindingPluginIsUnowned(pluginPath)) {
      return { ok: false, error: "refusing to overwrite an unmarked OpenCode plugin file" };
    }

    const json = (host.fs.fileExists(cfg)
      ? (host.fs.readJson<OpencodeConfig>(cfg) || { "$schema": "https://opencode.ai/config.json" })
      : { "$schema": "https://opencode.ai/config.json" }) as OpencodeConfig;
    if (!json || typeof json !== "object" || Array.isArray(json)) {
      return { ok: false, error: "opencode.json root must be an object" };
    }
    if (!Array.isArray(json.plugin)) json.plugin = [];
    json.plugin = json.plugin.filter((entry) => !opencodePluginEntryMatches(entry, target));
    json.plugin.push(target);

    if (!host.fs.writeFile(pluginPath, opencodeBindingPluginSource())) {
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
    if (pluginPath && existingBindingPluginIsUnowned(pluginPath)) {
      return { ok: false, error: "refusing to remove an unmarked OpenCode plugin file" };
    }
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

  ensureIntegrationOnStartup(_apiPort: number): unknown {
    return this.checkIntegration!();
  },

  sessionExists(_cwd: string, sessionId: string): boolean {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return false;
    const rows = host.fs.sqliteQuery<{ id: string }>(
      db,
      "SELECT id FROM session WHERE id = ?1 LIMIT 1",
      [sessionId]
    );
    return rows.some((row) => row.id === sessionId);
  },

  // opencode keeps every session in one sqlite log, so the source carries the
  // session id: the reader must not charge a sibling's tokens to this tab.
  usageSources(_cwd: string, sessionId: string): { path: string; sessionId?: string }[] {
    const db = opencodeDbPath();
    if (!db || !sessionId || !host.fs.fileExists(db)) return [];
    return [{ path: db, sessionId }];
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
      "SELECT data FROM message WHERE session_id = ?1 AND octet_length(data) <= ?2 " +
        "ORDER BY time_created DESC LIMIT 20",
      [sessionId, ASSISTANT_MESSAGE_MAX_BYTES]);
  },

  usesStructuredChat(): boolean { return true; },

  readStructuredChat(_cwd: string, sessionId: string, cursor?: string | null): unknown {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return { messages: [], cursor: cursor || null };
    const parsedCursor = parseStructuredCursor(cursor);
    if (!parsedCursor) return { messages: [], cursor: cursor ?? null };
    const [sinceMs, seenAtSinceMs] = parsedCursor;
    // Track `time_updated`, not `time_created`: a streamed part keeps its creation
    // instant while its text/output grows, so a creation window skips every update.
    const rows = host.fs.sqliteQuery<{ id: string | number; ts_created?: number; ts_updated?: number; part?: string; role?: string | null }>(
      db,
      "SELECT p.id AS id, p.time_created AS ts_created, p.time_updated AS ts_updated, p.data AS part, " +
        "CASE WHEN octet_length(m.data) <= ?3 AND json_valid(m.data) THEN json_extract(m.data,'$.role') END AS role " +
        "FROM part p JOIN message m ON m.id = p.message_id " +
        "WHERE p.session_id = ?1 AND p.time_updated >= ?2 " +
        "ORDER BY p.time_updated ASC, p.id ASC",
      [sessionId, sinceMs, MESSAGE_ROLE_MAX_BYTES]
    );
    const messages: unknown[] = [];
    let lastTs = sinceMs;
    let seenAtLastTs = seenAtSinceMs.slice();
    for (let i = 0; i < rows.length; i++) {
      const updated = rows[i].ts_updated || 0;
      const created = rows[i].ts_created || updated;
      const id = String(rows[i].id);
      if (updated > lastTs) {
        lastTs = updated;
        seenAtLastTs = [];
      }
      if (seenAtLastTs.indexOf(id) !== -1) continue;
      seenAtLastTs.push(id);
      const part = safeJson(rows[i].part);
      if (!part) continue;
      const role = typeof rows[i].role === "string" && rows[i].role ? rows[i].role as string : "assistant";
      const cm = partToChatMsg(part as OpencodePart, role, id, created);
      if (cm) messages.push(cm);
    }
    seenAtLastTs.sort();
    return { messages: messages, cursor: JSON.stringify([lastTs, seenAtLastTs]) };
  },
};

function resumeLaunchArgs(args: string[], sessionId: string): string[] {
  const db = opencodeDbPath();
  const model = db && host.fs.fileExists(db)
    ? scanMessagesForQualifiedModel(db, sessionId)
    : null;
  if (!model) return args.map(String);
  const retained: string[] = [];
  for (let i = 0; i < args.length; i++) {
    const arg = String(args[i]);
    if (arg === "-m" || arg === "--model") {
      i++;
      continue;
    }
    if (arg.startsWith("-m=") || arg.startsWith("--model=")) continue;
    retained.push(arg);
  }
  retained.push("-m", model);
  return retained;
}

function resumeLaunch(sessionId: string, skipPermissions?: boolean): { parts: string[]; directory: string | null } {
  const platform = host.platform();
  const db = opencodeDbPath();
  const directory = db && host.fs.fileExists(db) ? scanSessionDirectory(db, sessionId) : null;
  const parts = ["opencode"];
  if (directory) parts.push(host.shell.quoteFor(directory, platform));
  parts.push("--session", host.shell.quoteFor(sessionId, platform));
  if (skipPermissions) parts.push("--dangerously-skip-permissions");
  return { parts, directory };
}

export default plugin;
