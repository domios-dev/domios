/**
 * Opencode plugin contract + detection tests.
 * Run: npx tsx plugins/provider-opencode/tests/plugin.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import plugin from "../src/index";
import {
  isCurrentOpencodeBindingPluginSource,
  opencodeBindingPluginSource,
  ownsOpencodeBindingPluginSource,
  ASSISTANT_MESSAGE_MAX_BYTES,
} from "../src/helpers";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`); }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

console.log("\n--- meta ---\n");
eq(plugin.meta.id, "opencode", "meta.id is 'opencode'");
eq(plugin.meta.displayName, "OpenCode", "meta.displayName");

console.log("\n--- detect ---\n");

assert(plugin.detect({ bytes: "", title: "opencode" }), "title 'opencode' positive");
assert(plugin.detect({ bytes: "", title: "OPENCODE-x" }), "title uppercase positive (case-insensitive)");
assert(plugin.detect({ bytes: "Running opencode v1.2" }), "PTY 'opencode' substring positive");
assert(plugin.detect({ bytes: "OPENCODE banner" }), "PTY 'OPENCODE' substring positive");
assert(!plugin.detect({ bytes: "$ ls\n", title: "bash" }), "plain shell negative");
assert(!plugin.detect({ bytes: "", title: "codex" }), "codex title is not opencode");

console.log("\n--- parseLine ---\n");
eq(plugin.parseLine('{"x":1}'), [], "parseLine returns empty (no JSONL support)");
eq(plugin.parseLine(""), [], "empty line → empty events");

console.log("\n--- statusOf ---\n");
eq(plugin.statusOf({ kind: "anything" }), null, "statusOf always returns null");

console.log("\n--- actions ---\n");
eq(plugin.actions, [], "no actions defined yet");

console.log("\n--- titlePattern + noisePatterns ---\n");
assert(plugin.titlePattern instanceof RegExp, "titlePattern is RegExp");
assert(plugin.titlePattern!.test("opencode"), "titlePattern matches 'opencode'");
assert(Array.isArray(plugin.noisePatterns) && plugin.noisePatterns.length > 0, "noisePatterns non-empty");

console.log("\n--- binding nonce (install + outdated detection) ---\n");
const bindingPluginSource = opencodeBindingPluginSource();
assert(isCurrentOpencodeBindingPluginSource(bindingPluginSource), "generated binding plugin is current");
const previousBindingPluginSource = bindingPluginSource.replace(
  "codeterm-opencode-session-binding-v4",
  "codeterm-opencode-session-binding-v3",
);
assert(ownsOpencodeBindingPluginSource(previousBindingPluginSource), "older CodeTerm binding versions remain owned");
assert(!isCurrentOpencodeBindingPluginSource(previousBindingPluginSource), "older binding version is stale");
assert(!ownsOpencodeBindingPluginSource("// foreign-opencode-plugin\nconst value = true;"), "foreign plugin header is not owned");
assert(
  !isCurrentOpencodeBindingPluginSource(`// foreign-opencode-plugin\n${bindingPluginSource}`),
  "currentness requires the exact CodeTerm header",
);
assert(
  !ownsOpencodeBindingPluginSource("// codeterm-opencode-session-binding-vendor\nconst value = true;"),
  "non-version marker suffix is not owned",
);

async function loadGeneratedBindingHooks() {
  const requests: Array<{ session_id: string; source: string }> = [];
  const source = opencodeBindingPluginSource().replace(
    "export default async function CodeTermSessionBindingPlugin",
    "async function CodeTermSessionBindingPlugin",
  );
  const context = {
    process: {
      env: {
        CODETERM_TAB_ID: "tab-1",
        CODETERM_API_TOKEN: "token",
        CODETERM_SESSION_BINDING_NONCE: "nonce",
        CODETERM_API_URL: "http://dev",
      },
    },
    fetch: async (_url: string, options: { body: string }) => {
      const body = JSON.parse(options.body) as { session_id?: unknown; source?: unknown };
      if (typeof body.session_id === "string" && typeof body.source === "string") {
        requests.push({ session_id: body.session_id, source: body.source });
      }
      return { ok: true };
    },
    module: { exports: undefined as unknown },
  };
  new vm.Script(`${source}\nmodule.exports = CodeTermSessionBindingPlugin;`).runInNewContext(context);
  const makePlugin = context.module.exports as (input: { directory: string }) => Promise<{
    event: (input: { event: unknown }) => Promise<void>;
    "chat.message": (input: { sessionID: string }) => Promise<void>;
    "tool.execute.before": (input: { sessionID: string }) => Promise<void>;
    "shell.env": (input: { sessionID: string }, output: unknown) => Promise<unknown>;
  }>;
  const hooks = await makePlugin({ directory: "/work" });
  return { hooks, requests };
}

console.log("\n--- session binding deduplicates by active session ---\n");
{
  const { hooks, requests } = await loadGeneratedBindingHooks();
  const sameSession = "ses_existing";
  await hooks.event({ event: { type: "session.created", properties: { sessionID: sameSession } } });
  await hooks.event({ event: { type: "session.updated", properties: { sessionID: sameSession } } });
  await hooks.event({ event: { type: "tui.session.select", properties: { sessionID: sameSession } } });
  await hooks.event({ event: { type: "message.updated", properties: { sessionID: sameSession } } });
  await hooks.event({ event: { type: "message.part.updated", properties: { sessionID: sameSession } } });
  await hooks["chat.message"]({ sessionID: sameSession });
  await hooks["tool.execute.before"]({ sessionID: sameSession });
  const output = { OPENCODE_TEST_ENV: "1" };
  eq(await hooks["shell.env"]({ sessionID: sameSession }, output), output, "shell hook preserves its output");
  eq(
    requests.map(({ session_id, source: event }) => ({ session_id, source: event })),
    [{ session_id: sameSession, source: "session.created" }],
    "repeated event and hook reports for one session make one POST",
  );
  const changedSession = "ses_switched";
  await hooks.event({ event: { type: "session.updated", properties: { sessionID: changedSession } } });
  await hooks.event({ event: { type: "message.part.updated", properties: { sessionID: changedSession } } });
  eq(
    requests.map(({ session_id }) => session_id),
    [sameSession, changedSession],
    "a session change creates one new binding POST",
  );
}

{
  const resumed = await loadGeneratedBindingHooks();
  await resumed.hooks.event({
    event: { type: "message.part.updated", properties: { sessionID: "ses_resumed" } },
  });
  eq(
    resumed.requests.map(({ session_id }) => session_id),
    ["ses_resumed"],
    "the first event in a resumed provider process rebinds its existing session",
  );
}

function loadOpencodeWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const ocHost = (over: Record<string, unknown>) => ({
  manifestJson: () => JSON.stringify({
    models: [
      { id: "seed-opencode", displayName: "Seed OpenCode", description: "Manifest fallback" },
    ],
  }),
  homeDir: () => "/tmp",
  platform: () => "linux",
  expandHome: (v: string) => v,
  envGet: () => null,
  exec: () => JSON.stringify({ code: 1, stdout: "", stderr: "not available" }),
  unixNowMs: () => 1_000_000,
  fetch: () => JSON.stringify({ error: "no network in tests" }),
  log: () => {},
  shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
  fs: { fileExists: () => false, readJson: () => null, readFile: () => null },
  ...over,
});

console.log("\n--- cwd-scoped session detection ---\n");
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: () => [
        { id: "ses_foreign", directory: "/other/proj" },
        { id: "ses_owned", directory: "/work/proj" },
      ],
    },
  }));
  eq(p.detectSessionId("/work/proj", [], 300000), "ses_owned", "detectSessionId rejects a newer cross-cwd session");
  eq(p.detectSessionId("/missing/proj", [], 300000), null, "detectSessionId refuses when cwd has no candidate");
}

console.log("\n--- platform-safe session directory normalization ---\n");
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const rows = [
    { id: "ses_slash", directory: "/work/a/b" },
    { id: "ses_backslash", directory: "/work/a\\b" },
    { id: "ses_root", directory: "/" },
  ];
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: () => rows,
    },
  }));
  eq(p.detectSessionId("/work/a\\b", [], 300000), "ses_backslash", "POSIX backslash remains a legal distinct path byte");
  eq(p.detectSessionId("/", [], 300000), "ses_root", "POSIX root is preserved");

  const windows = loadOpencodeWithHost(ocHost({
    platform: () => "windows",
    fs: {
      fileExists: () => true,
      sqliteQuery: () => [
        { id: "ses_drive_relative", directory: "C:" },
        { id: "ses_drive_root", directory: "C:/" },
      ],
    },
  }));
  eq(windows.detectSessionId("C:\\", [], 300000), "ses_drive_root", "Windows drive root is not aliased to drive-relative cwd");
}

console.log("\n--- restored session lookup follows the configured session store ---\n");
{
  const dbPath = "C:\\Users\\tester\\opencode-profile\\restored.db";
  const savedSession = "ses_restore_owner";
  const sessions = new Set([savedSession]);
  const queries: Array<{ db: string; sql: string; params: unknown[] }> = [];
  const p = loadOpencodeWithHost(ocHost({
    platform: () => "windows",
    envGet: (name: string) => name === "OPENCODE_DB" ? dbPath : null,
    fs: {
      fileExists: (candidate: string) => candidate === dbPath,
      sqliteQuery: (db: string, sql: string, params: unknown[]) => {
        queries.push({ db, sql, params });
        const sessionId = params[0];
        return sessions.has(String(sessionId)) ? [{ id: sessionId }] : [];
      },
    },
  }));
  eq(p.sessionExists("/work/proj", savedSession), true, "saved session exists in OPENCODE_DB");
  eq(p.sessionExists("/work/proj", "ses_sibling"), false, "another session cannot prove the saved id");
  eq(
    queries.map(({ db, sql, params }) => ({ db, sql, params })),
    [
      { db: dbPath, sql: "SELECT id FROM session WHERE id = ?1 LIMIT 1", params: [savedSession] },
      { db: dbPath, sql: "SELECT id FROM session WHERE id = ?1 LIMIT 1", params: ["ses_sibling"] },
    ],
    "lookup queries the configured database by the exact tab session id",
  );
}

console.log("\n--- exclusive structured cursor ---\n");
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const rows: Array<Record<string, unknown>> = [
    { id: "p1", ts_created: 1000, ts_updated: 1000, part: JSON.stringify({ type: "text", text: "before boundary" }), role: "user" },
  ];
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: (_db: string, _sql: string, params: unknown[]) => {
        const sinceTs = Number(params[1] || 0);
        return rows.filter((row) => Number(row.ts_updated) >= sinceTs);
      },
    },
  }));
  const first = p.readStructuredChat("/work/proj", "ses_owned", null) as { messages: unknown[]; cursor: string };
  eq(first.cursor, '[1000,["p1"]]', "cursor includes timestamp and seen peer ids");
  rows.push({ id: "p0", ts_created: 1000, ts_updated: 1000, part: JSON.stringify({ type: "text", text: "same timestamp peer" }), role: "user" });
  const peer = p.readStructuredChat("/work/proj", "ses_owned", first.cursor) as { messages: Array<{ content: string }>; cursor: string };
  eq(peer.messages.map((message) => message.content), ["same timestamp peer"], "same-timestamp unseen peer is retained without replay");
  const empty = p.readStructuredChat("/work/proj", "ses_owned", peer.cursor) as { messages: unknown[] };
  eq(empty.messages, [], "cursor excludes every previously returned part");
}

console.log("\n--- streamed part growth is re-delivered (same id, newer update) ---\n");
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const rows: Array<Record<string, unknown>> = [
    { id: "p1", ts_created: 1000, ts_updated: 1000, part: JSON.stringify({ type: "text", text: "Hel" }), role: "assistant" },
  ];
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: (_db: string, _sql: string, params: unknown[]) => {
        const sinceTs = Number(params[1] || 0);
        return rows.filter((row) => Number(row.ts_updated) >= sinceTs);
      },
    },
  }));
  const first = p.readStructuredChat("/work/proj", "ses_owned", null) as { messages: Array<{ content: string }>; cursor: string };
  eq(first.messages.map((message) => message.content), ["Hel"], "first read delivers the partial part");
  // The part keeps its id and creation instant while its text grows.
  rows[0] = { ...rows[0], ts_updated: 1200, part: JSON.stringify({ type: "text", text: "Hello world" }) };
  const grown = p.readStructuredChat("/work/proj", "ses_owned", first.cursor) as { messages: Array<{ content: string; id: string }>; cursor: string };
  eq(grown.messages.map((message) => message.content), ["Hello world"], "grown content is re-delivered past the cursor");
  eq(grown.messages.map((message) => message.id), ["p1"], "the grown row keeps its part id");
  const settled = p.readStructuredChat("/work/proj", "ses_owned", grown.cursor) as { messages: unknown[] };
  eq(settled.messages, [], "an unchanged part is not replayed");
}

console.log("\n--- malformed structured cursor fails closed ---\n");
{
  let queryCalls = 0;
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: () => true,
      sqliteQuery: () => {
        queryCalls++;
        return [{ id: "history", ts: 1, part: JSON.stringify({ type: "text", text: "history" }), role: "user" }];
      },
    },
  }));
  for (const cursor of ["1000", "not-json", '[1000,"legacy-id"]']) {
    const result = p.readStructuredChat("/work/proj", "ses_owned", cursor) as { messages: unknown[]; cursor: string };
    eq(result.messages, [], `cursor ${cursor} replays no history`);
    eq(result.cursor, cursor, `cursor ${cursor} is preserved on refusal`);
  }
  eq(queryCalls, 0, "malformed and legacy cursors never query the store");
}

console.log("\n--- discoverModels ---\n");

{
  const p = loadOpencodeWithHost(ocHost({
    platform: () => "windows",
    shell: {
      quoteFor: (v: unknown, platform?: unknown) => `Q(${platform}:${String(v)})`,
      quote: (v: unknown) => `BAD(${String(v)})`,
    },
  }));
  const launch = p.buildLaunchCommand({
    sessionId: "sess 1",
    structuredPromptServer: { hostname: "127.0.0.1", port: 4567 },
  });
  assert(launch.includes("opencode --hostname Q(windows:127.0.0.1) --port Q(windows:4567) --session Q(windows:sess 1)"), "buildLaunchCommand quotes --session with quoteFor(platform)");
  assert(p.buildResumeCommand("sess 2").includes("opencode --session Q(windows:sess 2)"), "buildResumeCommand selects the full TUI for session resume");
  assert(p.buildLaunchCommand({ skipPermissions: true }).includes("opencode --dangerously-skip-permissions"), "buildLaunchCommand applies the centralized full-access policy");
  assert(p.buildResumeCommand("sess 2", true).includes("opencode --session Q(windows:sess 2) --dangerously-skip-permissions"), "buildResumeCommand applies the centralized full-access policy");
}

console.log("\n--- per-launch pad-only external directory permission ---\n");
{
  const unix = loadOpencodeWithHost(ocHost({
    platform: () => "linux",
    shell: { quoteFor: (value: unknown) => `'${String(value)}'`, quote: (value: unknown) => `'${String(value)}'` },
    fs: { writeTempFile: () => "/tmp/context.md", fileExists: () => false },
  }));
  const fresh = unix.buildLaunchCommand({});
  const restored = unix.buildResumeCommand("ses_restore");
  const resumed = unix.buildResumeCommandWithContext({ sessionId: "ses_resume", systemPrompt: "opaque prompt" });
  for (const [name, command] of [["fresh", fresh], ["restore", restored], ["resume", resumed]] as const) {
    assert(command.includes('"*":"ask"'), `${name} keeps unmatched external paths at ask`);
    assert(command.includes("CODETERM_PAD_DIR"), `${name} derives the grant from the tab launch environment`);
    assert(command.includes('if [ -n "$ct_pad_dir" ]'), `${name} grants nothing when the pad environment value is empty`);
    assert(command.includes('\\"$ct_pad_dir/*\\":\\"allow\\"'), `${name} allows only the normalized pad subtree`);
    assert(command.includes("unset OPENCODE_CONFIG_CONTENT"), `${name} clears the per-launch config after OpenCode exits`);
    assert(!command.includes(".config/opencode") && !command.includes("opencode.json"), `${name} does not write a global config path`);
  }
  assert(fresh.includes("tr '\\\\' '/'") && fresh.includes("sed 's/\"/\\\\\\\"/g'"), "Unix launch normalizes Windows separators and JSON-quotes the pad path");

  const posixConfig = (command: string) => {
    const prefix = command.match(/OPENCODE_CONFIG_CONTENT='([^']*)';/);
    const suffix = command.match(/\$\(printf '%s' '([^']*)'\); export OPENCODE_CONFIG_CONTENT/);
    assert(!!prefix && !!suffix, "POSIX inline config has safely quoted JSON prefix and suffix");
    return JSON.parse(`${prefix?.[1]},"/runtime/pads/tab-1/*":"allow"${suffix?.[1]}`) as { permission: { external_directory: Record<string, string> }; instructions?: string[] };
  };
  const parsedFresh = posixConfig(fresh);
  eq(parsedFresh.permission.external_directory, { "*": "ask", "/runtime/pads/tab-1/*": "allow" }, "POSIX fresh config grants only the tab pad and asks for all other paths");
  eq(posixConfig(restored).permission.external_directory, { "*": "ask", "/runtime/pads/tab-1/*": "allow" }, "POSIX restore config grants only the tab pad and asks for all other paths");
  const parsedResume = posixConfig(resumed);
  eq(parsedResume.instructions, ["/tmp/context.md"], "POSIX resume config references the host task file directly");
  assert(resumed.includes("opencode --session 'ses_resume'") && !resumed.includes("cp --"), "POSIX resume launches without staging instructions into the project");
  assert(!fresh.includes("exit $ct_status") && !restored.includes("exit $ct_status"), "plain launch and restore keep the tab shell open after OpenCode exits");
  const windows = loadOpencodeWithHost(ocHost({ platform: () => "windows", shell: { quoteFor: (v: unknown) => `'${String(v)}'`, quote: (v: unknown) => `'${String(v)}'` }, fs: { writeTempFile: () => "C:\\Temp\\context.md", fileExists: () => false } }));
  const windowsFresh = windows.buildLaunchCommand({});
  const windowsRestore = windows.buildResumeCommand("ses_restore");
  const windowsResume = windows.buildResumeCommandWithContext({ sessionId: "ses_resume", systemPrompt: "opaque prompt" });
  const windowsPermission = (name: string, command: string) => {
    assert(command.includes("([string]$env:CODETERM_PAD_DIR).TrimEnd"), `Windows ${name} reads the tab pad environment`);
    assert(command.includes("$ct_pad = $ct_pad_native -replace"), `Windows ${name} normalizes the pad path`);
    assert(command.includes("if ($ct_pad_native)"), `Windows ${name} grants nothing for an empty pad`);
    const configText = command.match(/\$ct_config = '([^']*)' \| ConvertFrom-Json/)?.[1];
    assert(!!configText, `Windows ${name} embeds a quoted config`);
    const config = JSON.parse(configText || "{}") as { permission: { external_directory: Record<string, string> } };
    eq(config.permission.external_directory["*"], "ask", `Windows ${name} leaves other external paths at ask`);
    const patternAdds = [...command.matchAll(/Add-Member -NotePropertyName \((\$ct_pad_native|\$ct_pad) \+ '([^']+)'\) -NotePropertyValue 'allow' -Force/g)];
    eq(patternAdds.map((m) => [m[1], m[2]]), [["$ct_pad_native", "\\*"], ["$ct_pad", "/*"]], `Windows ${name} adds exactly the native and forward-slash pad patterns`);
    assert(command.includes("-replace '\\\\', '/'"), `Windows ${name} produces OpenCode's forward-slash path form`);
  };
  for (const [name, command] of [["fresh", windowsFresh], ["restore", windowsRestore], ["resume", windowsResume]] as const) {
    windowsPermission(name, command);
    assert(command.includes("Remove-Item Env:OPENCODE_CONFIG_CONTENT"), `Windows ${name} clears the per-launch config after OpenCode exits`);
    assert(!command.includes(".config/opencode") && !command.includes("opencode.json"), `Windows ${name} does not write a global config path`);
  }
  const windowsConfigText = windowsResume.match(/\$ct_config = '([^']*)' \| ConvertFrom-Json/)?.[1];
  const parsedWindows = JSON.parse(windowsConfigText || "{}") as { instructions?: string[] };
  eq(parsedWindows.instructions, ["C:\\Temp\\context.md"], "Windows resume config references the host task file directly");
  assert(windowsResume.includes("opencode --session 'ses_resume'") && !windowsResume.includes("Copy-Item"), "Windows resume launches without staging instructions into the project");
  for (const [name, command] of [["fresh", windowsFresh], ["restore", windowsRestore]] as const) {
    const embedded = command.match(/\$ct_config = '([^']*)' \| ConvertFrom-Json/)?.[1];
    assert(!!embedded, `Windows ${name} embeds a quoted config`);
    eq(JSON.parse(embedded || "{}").permission.external_directory, { "*": "ask" }, `Windows ${name} base config is valid JSON with unmatched paths asking`);
    assert(command.includes("try { opencode"), `Windows ${name} executes opencode outside any copy command`);
  }
}

console.log("\n--- restored full-TUI command keeps the saved project, session, and model ---\n");
{
  const dbPath = "C:\\Users\\tester\\opencode-profile\\restored.db";
  const savedSession = "ses_saved";
  const queries: Array<{ sql: string; params: unknown[] }> = [];
  const p = loadOpencodeWithHost(ocHost({
    platform: () => "windows",
    envGet: (name: string) => name === "OPENCODE_DB" ? dbPath : null,
    fs: {
      fileExists: (candidate: string) => candidate === dbPath,
      writeTempFile: () => "C:\\Temp\\restored-context.md",
      sqliteQuery: (_db: string, sql: string, params: unknown[]) => {
        queries.push({ sql, params });
        if (sql.startsWith("SELECT id, directory FROM session") && params[0] === savedSession) {
          return [
            { id: "ses_sibling", directory: "D:/other/project" },
            { id: savedSession, directory: "D:/Developer/codeterm-livetest" },
          ];
        }
        if (sql.startsWith("SELECT data FROM message") && params[0] === savedSession) {
          return [{ data: JSON.stringify({ role: "assistant", providerID: "opencode", modelID: "big-pickle" }) }];
        }
        return [];
      },
    },
    shell: {
      quoteFor: (value: unknown, platform?: unknown) => `Q(${platform}:${String(value)})`,
      quote: (value: unknown) => `BAD(${String(value)})`,
    },
  }));
  const plainResume = p.buildResumeCommandWithContext({
      sessionId: savedSession,
      args: ["-m", "big-pickle", "--agent", "build"],
    });
  eq(
    plainResume.match(/try \{ (opencode .+?) \} finally/)?.[1],
    "opencode Q(windows:D:/Developer/codeterm-livetest) --session Q(windows:ses_saved) --agent Q(windows:build) -m Q(windows:opencode/big-pickle)",
    "resume selects the saved project and exact session in the full TUI inside the config wrapper",
  );
  eq(
    queries,
    [
      {
        sql: "SELECT id, directory FROM session WHERE id = ?1 LIMIT 1",
        params: [savedSession],
      },
      {
        sql: "SELECT data FROM message WHERE session_id = ?1 AND octet_length(data) <= ?2 ORDER BY time_created DESC LIMIT 20",
        params: [savedSession, ASSISTANT_MESSAGE_MAX_BYTES],
      },
    ],
    "resume project and model lookups are scoped to the tab's saved session id",
  );
  const configured = p.buildResumeCommandWithContext({
    sessionId: savedSession,
    systemPrompt: "opaque-context-fragment",
  });
  assert(
    configured.includes("Set-Location -LiteralPath Q(windows:D:/Developer/codeterm-livetest) -ErrorAction Stop;") && !configured.includes("New-Item"),
    "resume stops if it cannot enter the saved project directory and does not stage task files",
  );
  assert(
    configured.includes("try { Set-Location -LiteralPath Q(windows:D:/Developer/codeterm-livetest) -ErrorAction Stop; opencode Q(windows:D:/Developer/codeterm-livetest) --session Q(windows:ses_saved)") &&
      !configured.includes("Copy-Item"),
    "session-config wrapper preserves the explicit full-TUI resume argv",
  );
  const configuredConfig = JSON.parse(configured.match(/\$ct_config = Q\(windows:(.*?)\) \| ConvertFrom-Json/)?.[1] || "{}") as { instructions?: string[] };
  eq(configuredConfig.instructions, ["C:\\Temp\\restored-context.md"], "resume config references the host context file directly");
  assert(!configured.includes("--prompt"), "resume restores session instructions without submitting a new launch turn");
}

console.log("\n--- captured full-TUI resume screen matches provider readiness fingerprints ---\n");
{
  const fixture = fs.readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "resume-full-tui-screen.txt"),
    "utf8",
  );
  const p = loadOpencodeWithHost(ocHost({}));
  eq(p.screenHasTui(fixture), true, "captured full TUI exposes provider readiness footer fingerprints");
  eq(p.detectFromOutput(fixture), true, "captured full TUI remains identifiable as OpenCode output");
}

{
  const p = loadOpencodeWithHost(ocHost({
    platform: () => "windows",
    fs: { writeTempFile: () => "C:\\Temp\\opencode task.md", fileExists: () => false },
    shell: { quoteFor: (v: unknown) => `'${String(v)}'`, quote: (v: unknown) => `'${String(v)}'` },
  }));
  const hostileTask = "line one\n<location>D:\\repo</location>\n'quotes' \"double\" $HOME `whoami` --flag\nюникод";
  const launch = p.buildLaunchCommand({ task: hostileTask });
  assert(launch.includes("$env:OPENCODE_CONFIG_CONTENT ="), "OpenCode passes task instructions through isolated config");
  const launchConfig = JSON.parse(launch.match(/\$ct_config = '([^']*)' \| ConvertFrom-Json/)?.[1] || "{}") as { instructions?: string[] };
  eq(launchConfig.instructions, ["C:\\Temp\\opencode task.md"], "OpenCode config references the host task file");
  assert(!launch.includes("Copy-Item") && !launch.includes("mkdir -p .codeterm") && !launch.includes("New-Item -ItemType Directory -Force -Path '.codeterm'"), "OpenCode does not create task files in the project");
  assert(launch.includes("--prompt "), "OpenCode submits the host-selected starter turn");
  assert(!launch.includes(hostileTask), "OpenCode task body is absent from the launch command");
  const idle = p.buildLaunchCommand({ task: hostileTask, starterPrompt: "idle_wakeup" });
  const idleStarter = p.starterPromptText("idle_wakeup");
  const taskStarter = p.starterPromptText("assigned_task");
  assert(!!idleStarter && idle.includes(`--prompt '${idleStarter}'`), "OpenCode idle launch submits the idle wake-up starter");
  assert(!!taskStarter && !idle.includes(taskStarter), "OpenCode idle launch never uses the task starter");
  const silent = p.buildLaunchCommand({ task: hostileTask, starterPrompt: "no_starter" });
  assert(!silent.includes("--prompt"), "OpenCode silent launch has no starter turn");
  const teamText = "host-owned team bootstrap";
  const team = p.buildLaunchCommand({ task: hostileTask, starterPrompt: "team_bootstrap", starterPromptText: teamText });
  assert(team.includes(teamText), "OpenCode team launch uses the host-owned starter text");
  const handshakeText = "opaque-startup-handshake";
  const handshake = p.buildLaunchCommand({ task: hostileTask, starterPrompt: "startup_handshake", starterPromptText: handshakeText });
  assert(handshake.includes(handshakeText), "OpenCode startup launch uses the host-owned starter text");
  eq(p.starterPromptText?.("team_bootstrap"), undefined, "OpenCode does not duplicate the team starter");
}

{
  const p = loadOpencodeWithHost(ocHost({
    exec: () => JSON.stringify({ code: 0, stdout: "opencode/big-pickle\nanthropic/claude-opus-4-8\n" }),
  }));
  eq(
    p.discoverModels(),
    [
      { id: "opencode/big-pickle", displayName: "OpenCode Zen Big Pickle", group: "OpenCode Zen" },
      { id: "anthropic/claude-opus-4-8", displayName: "Claude Opus 4.8", group: "anthropic" },
    ],
    "discoverModels: clean labels (OpenCode Zen prefix + dotted version), no availability signal → unset",
  );
}

// Non-chat (embedding/tts) ids dropped from the live list.
{
  const p = loadOpencodeWithHost(ocHost({
    exec: () => JSON.stringify({ code: 0, stdout: "openai/text-embedding-3-large\nopencode/gpt-5.5\nopenai/whisper-1\n" }),
  }));
  eq(
    p.discoverModels(),
    [{ id: "opencode/gpt-5.5", displayName: "OpenCode Zen GPT 5.5", group: "OpenCode Zen" }],
    "embedding + whisper filtered out; chat model kept with clean label",
  );
}

{
  const p = loadOpencodeWithHost(ocHost({}));
  eq(
    p.discoverModels(),
    null,
    "discoverModels returns null when the opencode models probe is unavailable, so the host keeps its seed",
  );
}

// Image/video generation ids are not chat models.
{
  const p = loadOpencodeWithHost(ocHost({
    exec: () => JSON.stringify({ code: 0, stdout: "xai/grok-4.6\nxai/grok-imagine-image\nxai/grok-imagine-video-1.5\n" }),
  }));
  eq(
    p.discoverModels(),
    [{ id: "xai/grok-4.6", displayName: "Grok 4.6", group: "xai" }],
    "image/video generation models are filtered; the chat model is kept",
  );
}

// Per-group usage is read from opencode's store and keyed by providerID.
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: () => [
        { g: "deepseek", messages: 2, input: 100, output: 40, cache_read: 10, cache_write: 0, cost: 1.9425 },
        { g: null, messages: 9, input: 1, output: 1, cache_read: 0, cache_write: 0, cost: 0 },
      ],
    },
  }));
  eq(
    p.modelGroupUsage(),
    [{
      group: "deepseek",
      messages: 2,
      input_tokens: 100,
      output_tokens: 40,
      cache_read_tokens: 10,
      cache_write_tokens: 0,
      cost_micros: 1_942_500,
      spend_cents: 194,
    }],
    "modelGroupUsage maps rows to snake_case groups, adds observed spend, and drops a null provider",
  );
}

// A group with a live balance endpoint carries remaining credit; the limit is
// left absent because the endpoint exposes no included allowance.
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  let fetched = "";
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: () => [
        { g: "deepseek", messages: 1, input: 10, output: 5, cache_read: 0, cache_write: 0, cost: 3.39 },
        { g: "xai", messages: 1, input: 10, output: 5, cache_read: 0, cache_write: 0, cost: 118.75 },
      ],
    },
    fetch: (opts: string) => {
      fetched = opts;
      return JSON.stringify({
        status: 200,
        body: JSON.stringify({
          is_available: true,
          balance_infos: [{ currency: "USD", total_balance: "0.62", topped_up_balance: "0.62" }],
        }),
      });
    },
  }));
  const rows = p.modelGroupUsage() as Array<Record<string, unknown>>;
  const deepseek = rows.find((r) => r.group === "deepseek");
  const xai = rows.find((r) => r.group === "xai");
  assert(
    fetched.indexOf("api.deepseek.com") !== -1 &&
      fetched.indexOf("opencodeAuth") !== -1 &&
      fetched.indexOf("deepseekKey") !== -1,
    "the balance read names its own URL and the declared credential secret",
  );
  eq(deepseek?.spend_remaining_cents, 62, "deepseek remaining credit is read live");
  eq(deepseek?.spend_limit_cents, undefined, "no included limit is invented");
  eq(xai?.spend_cents, 11875, "xai spend comes from its own observed cost");
  eq(xai?.spend_remaining_cents, undefined, "a group without a balance endpoint has no remaining");
  eq(xai?.spend_limit_cents, undefined, "a group without a live limit reports null, never a guess");
}

// A failed balance read still reports the observed spend, with no fabricated limit.
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: () => [
        { g: "deepseek", messages: 1, input: 10, output: 5, cache_read: 0, cache_write: 0, cost: 1.0 },
      ],
    },
    fetch: () => JSON.stringify({ error: "request failed: timed out" }),
  }));
  const rows = p.modelGroupUsage() as Array<Record<string, unknown>>;
  eq(rows[0].spend_cents, 100, "observed spend survives a failed balance read");
  eq(rows[0].spend_remaining_cents, undefined, "an unreadable balance leaves remaining absent");
}

{
  const p = loadOpencodeWithHost(ocHost({ fs: { fileExists: () => false } }));
  eq(p.modelGroupUsage(), [], "no session store ⇒ no usage, never a guess");
}

// The manifest declares the balance credential secret and the network host the
// plugin's balance read reaches — core never learns either.
{
  const manifest = JSON.parse(
    fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.json"), "utf8"),
  );
  const cred = (manifest.credentials || []).find((c: { id?: string }) => c.id === "opencodeAuth");
  assert(cred?.secret?.deepseekKey === "deepseek.key", "auth credential declares the deepseek key secret");
  assert(
    (manifest.permissions?.network?.allow || []).includes("api.deepseek.com"),
    "manifest allows the deepseek balance host",
  );
}

// opencode plugin so the session-start report can prove tab ownership.
{
  let body = "";
  const host = ocHost({
    fs: {
      fileExists: () => false,
      readJson: () => null,
      readFile: () => null,
      writeFile: (_p: string, content: string) => { body = content; return true; },
      writeJson: () => true,
    },
  });
  const p = loadOpencodeWithHost(host);
  const res = p.installIntegration(7686, { consent: true });
  assert(!!res && res.ok === true, "installIntegration succeeds", JSON.stringify(res));
  assert(
    body.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 &&
      body.indexOf("x-codeterm-binding-nonce") !== -1 &&
      body.indexOf("CODETERM_API_URL") !== -1 &&
      body.indexOf("CODETERM_API_TOKEN") !== -1 &&
      body.indexOf("CODETERM_TAB_ID") !== -1 &&
      body.indexOf("CODETERM_API_PORT") === -1 &&
      body.indexOf("FALLBACK_PORT") === -1 &&
      body.indexOf("127.0.0.1") === -1 &&
      body.indexOf("localhost") === -1,
    "generated hook routes only through its process environment",
  );
}

{
  let writes = 0;
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: () => false,
      readJson: () => null,
      readFile: () => null,
      writeFile: () => { writes += 1; return true; },
      writeJson: () => { writes += 1; return true; },
    },
  }));
  const result = p.installIntegration(7686, { consent: false });
  assert(!!result && result.ok === false && writes === 0, "integration mutation requires consent");
}

{
  const pluginBody = "// foreign-opencode-plugin\nconst value = true;";
  let fileWrites = 0;
  let configWrites = 0;
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: () => true,
      readFile: () => pluginBody,
      readJson: () => ({ plugin: [] }),
      writeFile: () => { fileWrites += 1; return true; },
      writeJson: () => { configWrites += 1; return true; },
    },
  }));
  const result = p.installIntegration(7686, { consent: true });
  assert(
    !!result && result.ok === false && fileWrites === 0 && configWrites === 0,
    "install refuses an existing unmarked plugin file before changing either file",
  );
}

{
  const pluginBody = "// foreign-opencode-plugin\nconst value = true;";
  let configWrites = 0;
  let fileRemovals = 0;
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: () => true,
      readFile: () => pluginBody,
      readJson: () => ({ plugin: [] }),
      writeJson: () => { configWrites += 1; return true; },
      removeFile: () => { fileRemovals += 1; return true; },
    },
  }));
  const result = p.uninstallIntegration();
  assert(
    !!result && result.ok === false && configWrites === 0 && fileRemovals === 0,
    "uninstall refuses an existing unmarked plugin file before changing either file",
  );
}

{
  let writes = 0;
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: () => true,
      readFile: () => "// codeterm-opencode-session-binding-v2\nCODETERM_API_PORT 7685",
      readJson: () => null,
      writeFile: () => { writes += 1; return true; },
      writeJson: () => { writes += 1; return true; },
    },
  }));
  const result = p.ensureIntegrationOnStartup(7686);
  assert(!!result && result.plugin_file_current === false && writes === 0, "reload inspection does not repair without core consent");
}

// A plugin file lacking the nonce is pre-nonce → not current (forces reinstall).
{
  const withBody = (body: string) =>
    ocHost({
      fs: {
        fileExists: () => true,
        readFile: () => body,
        readJson: () => null,
        writeFile: () => true,
        writeJson: () => true,
      },
    });
  const old = loadOpencodeWithHost(withBody("// codeterm-opencode-session-binding-v2\nconst x = 1;"));
  assert(
    old.checkIntegration().plugin_file_current === false,
    "pre-nonce opencode plugin reads as not-current",
  );
  assert(
    old.checkIntegration().plugin_file_owned === true,
    "older CodeTerm plugin versions remain owned and refreshable",
  );
  const foreignFile = loadOpencodeWithHost(withBody("// foreign-opencode-plugin\nconst x = 1;"));
  assert(foreignFile.checkIntegration().plugin_file_owned === false, "foreign plugin files are not refreshable as ours");
  const cur = loadOpencodeWithHost(
    withBody(
      "// codeterm-opencode-session-binding-v3\n" +
      "const base = process.env.CODETERM_API_URL;\n" +
      "const token = process.env.CODETERM_API_TOKEN;\n" +
      "const nonce = process.env.CODETERM_SESSION_BINDING_NONCE;\n" +
      "process.env.CODETERM_TAB_ID;\n" +
      'const endpoint = "/api/tabs/";\n' +
      "let lastReported = null;\n" +
      "sessionID === lastReported;\n" +
      '"x-codeterm-binding-nonce";\n' +
      "authorization;",
    ),
  );
  assert(
    cur.checkIntegration().plugin_file_current === true,
    "opencode plugin carrying the nonce reads as current",
  );
  assert(cur.checkIntegration().plugin_file_owned === true, "current plugin reports marker ownership");
  const stalePane = loadOpencodeWithHost(
    withBody(
      "// codeterm-opencode-session-binding-v2\n" +
      "process.env.CODETERM_SESSION_BINDING_NONCE;\n" +
      "process.env.CODETERM_PANE_ID;\n" +
      'const endpoint = "/api/panes/";',
    ),
  );
  assert(
    stalePane.checkIntegration().plugin_file_current === false,
    "nonce-bearing Pane-era opencode plugin reads as not-current",
  );
  const staleInstance = loadOpencodeWithHost(
    withBody(
      "// codeterm-opencode-session-binding-v2\n" +
      "process.env.CODETERM_API_URL || `http://127.0.0.1:${process.env.CODETERM_API_PORT || 7685}`;\n" +
      "process.env.CODETERM_API_TOKEN;\n" +
      "process.env.CODETERM_SESSION_BINDING_NONCE;\n" +
      "process.env.CODETERM_TAB_ID;\n" +
      'const endpoint = "/api/tabs/";\n' +
      '"x-codeterm-binding-nonce";\n' +
      "authorization;",
    ),
  );
  assert(staleInstance.checkIntegration().plugin_file_current === false, "instance-bound helper is stale");
  const foreign = loadOpencodeWithHost(withBody("const endpoint = process.env.CODETERM_API_URL;"));
  assert(foreign.checkIntegration().plugin_file_owned === false, "unmarked file is not CodeTerm-owned");
  const unreadableConfig = loadOpencodeWithHost(
    ocHost({
      fs: {
        fileExists: () => true,
        readFile: () => "// codeterm-opencode-session-binding-v2",
        readJson: () => null,
        writeFile: () => false,
        writeJson: () => false,
      },
    }),
  );
  assert(
    unreadableConfig.checkIntegration().integration_state === "unreadable",
    "an unreadable config reports a reconciliation state instead of absence",
  );
}

console.log("--- permission_request: opencode declared session state ---");
{
  const manifest = JSON.parse(fs.readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.json"),
    "utf8",
  ));
  const roster = manifest.spawn?.hookEventRoster as Array<{ event: string; route: string; verdict?: string }>;
  const entry = (event: string) => roster.find((r) => r.event === event);
  assert(Array.isArray(roster), "opencode declares a hook event roster in its manifest");
  eq(entry("SessionStart")?.route, "session_start", "SessionStart stays declared, not dropped");
  eq(entry("session.status")?.route, "session_state", "session.status lands on session_state");
  eq(entry("session.status")?.verdict, "working", "session.status is declared Working");
  eq(entry("session.idle")?.route, "session_state", "session.idle lands on session_state");
  eq(entry("session.idle")?.verdict, "idle", "session.idle is declared Idle");
  eq(entry("session.deleted"), undefined, "an undeclared opencode event is absent from the roster");
}

// never a "latest model in cwd" fallback that would surface a SIBLING's model.
console.log("\n--- detectSessionModel (identity-bound, no cwd fallback) ---\n");

{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (value: string) => value === dbPath,
      sqliteQuery: () => [{
        id: "ses_real",
        directory: "D:/Developer/codeterm",
        title: "Tool UI",
        time_created: 1781000000000,
        time_updated: 1781000005000,
      }],
    },
  }));
  eq(p.enumerateSessions(), [{
    sessionId: "ses_real",
    provider: "opencode",
    projectPath: "D:/Developer/codeterm",
    title: "Tool UI",
    summary: null,
    filePath: dbPath,
    createdTs: 1781000000,
    modifiedTs: 1781000005,
  }], "OpenCode sessions expose their SQLite directory for resume");
}

{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const queries: { sql: string; params: unknown[] }[] = [];
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (value: string) => value === dbPath,
      sqliteQuery: (_db: string, sql: string, params: unknown[]) => {
        queries.push({ sql, params });
        return params[0] === "ses_real" ? [{
          id: "ses_real",
          directory: "D:/Developer/codeterm",
          title: "Tool UI",
          time_created: 1781000000000,
          time_updated: 1781000005000,
        }] : [];
      },
    },
  }));
  eq(p.findSession("ses_real"), {
    sessionId: "ses_real",
    provider: "opencode",
    projectPath: "D:/Developer/codeterm",
    title: "Tool UI",
    summary: null,
    filePath: dbPath,
    createdTs: 1781000000,
    modifiedTs: 1781000005,
  }, "findSession answers from the one session row with the enumerated record shape");
  eq(p.findSession("ses_missing"), null, "an id without a session row is not owned");
  eq(p.findSession("01900000-0000-7000-8000-000000000001"), null, "a foreign id shape is refused");
  eq(queries.map((query) => query.params), [["ses_real"], ["ses_missing"]],
    "each lookup issues exactly one query keyed by the requested id");
  assert(queries.every((query) => /WHERE id = \?1 LIMIT 1/.test(query.sql) && query.sql.indexOf("ORDER BY") === -1),
    "exact lookup never scans the session table");
}

{
  const dbPath = "~/.local/share/opencode/opencode.db";
  let cwdScanRan = false;
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      readJson: () => null,
      readFile: () => null,
      writeFile: () => true,
      writeJson: () => true,
      sqliteQuery: (_db: string, sql: string, params: unknown[]) => {
        // s.directory — never by the tab's session_id.
        if (sql.indexOf("s.directory") !== -1 || sql.indexOf("JOIN session") !== -1) {
          cwdScanRan = true;
          return [{ data: JSON.stringify({ role: "assistant", modelID: "sibling/model" }) }];
        }
        // Per-session query bound to ?1 = sessionId, no matching rows.
        void params;
        return [];
      },
    },
  }));
  eq(
    p.detectSessionModel("/some/cwd", "ses_owns_no_model"),
    null,
    "no model row for THIS session → null (no cwd fallback to a sibling's model)",
  );
  assert(!cwdScanRan, "must NOT run the cwd-wide model scan");

  // Empty session id → null without touching the db scan.
  eq(p.detectSessionModel("/some/cwd", ""), null, "empty sessionId → null");

  // The happy path still works: this session's own assistant message wins.
  const own = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      readJson: () => null,
      readFile: () => null,
      writeFile: () => true,
      writeJson: () => true,
      sqliteQuery: () => [{ data: JSON.stringify({ role: "assistant", modelID: "anthropic/claude-own" }) }],
    },
  }));
  eq(own.detectSessionModel("/cwd", "ses_mine"), "anthropic/claude-own", "this session's own model is returned");
}

console.log("\n--- usageSources (session-scoped sqlite log) ---\n");
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const p = loadOpencodeWithHost(ocHost({
    fs: { fileExists: (pth: string) => pth === dbPath },
  }));
  eq(
    p.usageSources("/work/proj", "ses_owned"),
    [{ path: dbPath, sessionId: "ses_owned" }],
    "OpenCode declares the shared sqlite log scoped to the tab's own session",
  );
  eq(p.usageSources("/work/proj", ""), [], "an empty session id declares no source");

  const absent = loadOpencodeWithHost(ocHost({ fs: { fileExists: () => false } }));
  eq(absent.usageSources("/work/proj", "ses_owned"), [], "a missing db declares no source");
}

console.log("\n--- canonical tool fields ---\n");
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: (_db: string, sql: string) => {
        if (sql.indexOf("FROM part") !== -1) {
          return [
            { id: 1, ts: 1000, part: JSON.stringify({ type: "tool", tool: "Edit", state: { input: { filePath: "src/demo.ts", oldString: "old", newString: "new" } } }), role: "assistant" },
            { id: 2, ts: 1001, part: JSON.stringify({ type: "tool", tool: "MultiEdit", state: { input: { filePath: "src/multi.ts", edits: [{ oldString: "one", newString: "two" }, { oldString: "three", newString: "four" }] } } }), role: "assistant" },
            { id: 3, ts: 1002, part: JSON.stringify({ type: "tool", tool: "Mystery", state: { input: { value: "opaque" } } }), role: "assistant" },
          ];
        }
        return [];
      },
    },
  }));
  const parsed = p.readStructuredChat("/cwd", "session") as { messages: Array<Record<string, unknown>> };
  eq(parsed.messages[0].toolKind, "edit", "OpenCode Edit gets canonical edit kind");
  assert(typeof parsed.messages[0].toolDetail === "string", "OpenCode toolDetail stays a string");
  eq(parsed.messages[0].toolEdits, [{ path: "src/demo.ts", old: "old", new: "new" }], "OpenCode camelCase edit input becomes canonical path and pair");
  eq(parsed.messages[1].toolEdits, [
    { path: "src/multi.ts", old: "one", new: "two" },
    { path: "src/multi.ts", old: "three", new: "four" },
  ], "OpenCode MultiEdit propagates parent camelCase path");
  eq(parsed.messages[2].toolKind, "generic", "unknown OpenCode tool degrades to generic");
  assert(typeof parsed.messages[2].toolSummary === "string" && !(parsed.messages[2].toolSummary as string).includes("{"), "unknown OpenCode summary is not JSON");
}

console.log("\n--- parsePrompt (horizontal option row) ---\n");
{
  const p = loadOpencodeWithHost(ocHost({}));
  const screen = [
    "│ Permission required                       │",
    "│ Run `rm -rf build`?                        │",
    "│                                            │",
    "│   Allow once   Allow always   Reject       │",
  ].join("\n");
  const parsed = p.parsePrompt(screen);
  eq(parsed && parsed.question, "Run `rm -rf build`?", "parsePrompt extracts the question");
  eq(
    parsed && parsed.options,
    [["Allow once", "1"], ["Allow always", "2"], ["Reject", "3"]],
    "parsePrompt extracts ALL three horizontal options (1-based values)",
  );
  eq(p.parsePrompt("just some normal assistant output\nwith no permission box"), null, "non-prompt output → null");
}

console.log("\n--- parsePrompt (vertical numbered question picker) ---\n");
{
  const p = loadOpencodeWithHost(ocHost({}));
  const fixture = fs.readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "question-picker.txt"),
    "utf8",
  );
  const parsed = p.parsePrompt(fixture);
  eq(parsed && parsed.question, "Выбери, что хочешь увидеть:", "parsePrompt extracts the question picker header");
  assert(!!parsed && parsed.options.length >= 2, "parsePrompt extracts at least two question picker options");
  eq(
    parsed && parsed.options,
    [
      ["A — Исследовать код", "1"],
      ["Б — Подкорректировать файл", "2"],
      ["В — Запустить тесты/сборку", "3"],
      ["Г — Поискать в коде", "4"],
      ["Type your own answer", "5"],
    ],
    "parsePrompt extracts numbered question picker options",
  );
  assert(p.hasEventMarkers("↑↓ select  enter submit  esc dismiss"), "hasEventMarkers fires on question picker footer");
  assert(p.hasEventMarkers("2. Б — Подкорректировать файл"), "hasEventMarkers fires on numbered question picker row");
  eq(p.detectEvents(fixture), ["permission_request"], "detectEvents surfaces question picker as waiting input");
  eq(p.buildPromptResponse(2, fixture), ["G1tC", "G1tC", "DQ=="], "vertical without visible highlight falls back to top-start");

  const highlighted = fixture.replace("3. В — Запустить тесты/сборку", "❯ 3. В — Запустить тесты/сборку");
  p.parsePrompt(highlighted);
  eq(p.buildPromptResponse(4, highlighted), ["G1tC", "G1tC", "DQ=="], "vertical target below highlight → down ×2 + Enter");
  eq(p.buildPromptResponse(0, highlighted), ["G1tB", "G1tB", "DQ=="], "vertical target above highlight → up ×2 + Enter");
}

console.log("\n--- parsePrompt (opencode modal pickers) ---\n");
{
  const p = loadOpencodeWithHost(ocHost({}));
  const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "testbench", "snapshots", "opencode");
  const model = fs.readFileSync(path.join(root, "model-picker", "screen.txt"), "utf8");
  const modelParsed = p.parsePrompt(model);
  eq(modelParsed && modelParsed.question, "Select model", "model picker question");
  eq(
    modelParsed && modelParsed.options.slice(0, 5),
    [
      ["Big Pickle Free", "1"],
      ["DeepSeek V4 Flash Free Free", "2"],
      ["MiMo V2.5 Free Free", "3"],
      ["Nemotron 3 Ultra Free Free", "4"],
      ["North Mini Code Free Free", "5"],
    ],
    "model picker extracts visible model choices without sidebar rows",
  );
  eq(p.buildPromptResponse(4, model), ["G1tC", "G1tC", "G1tC", "DQ=="], "model picker sends down from highlighted row");

  const connect = fs.readFileSync(path.join(root, "connect-picker", "screen.txt"), "utf8");
  const connectParsed = p.parsePrompt(connect);
  eq(connectParsed && connectParsed.question, "Connect a provider", "connect picker question");
  eq(
    connectParsed && connectParsed.options,
    [
      ["OpenCode Zen (Recommended)", "1"],
      ["OpenCode Go Low cost subscription for everyone", "2"],
      ["OpenAI (ChatGPT Plus/Pro or API key)", "3"],
      ["GitHub Copilot", "4"],
      ["Anthropic (API key)", "5"],
      ["Google", "6"],
      ["302.AI", "7"],
      ["Abacus", "8"],
      ["abliteration.ai", "9"],
      ["AIHubMix", "10"],
      ["Alibaba", "11"],
    ],
    "connect picker extracts provider choices",
  );
}

console.log("\n--- buildPromptResponse (horizontal ←/→ nav, NOT down) ---\n");
{
  const p = loadOpencodeWithHost(ocHost({}));
  const screen = [
    "│ Permission required                       │",
    "│ Run `rm -rf build`?                        │",
    "│                                            │",
    "│   Allow once   Allow always   Reject       │",
  ].join("\n");
  p.parsePrompt(screen);
  // value=1 → optionIndex 0 → just Enter (leftmost already highlighted).
  eq(p.buildPromptResponse(0, screen), ["DQ=="], "index 0 → Enter only");
  // value=3 → optionIndex 2 → right, right, Enter (NOT down×2).
  eq(p.buildPromptResponse(2, screen), ["G1tD", "G1tD", "DQ=="], "index 2 → right ×2 + Enter (ESC[C, not ESC[B)");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
