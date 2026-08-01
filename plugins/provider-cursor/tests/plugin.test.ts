/**
 * Cursor plugin contract + detection tests.
 * Run: npx tsx plugins/provider-cursor/tests/plugin.test.ts
 */

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import plugin from "../src/index";

const fixturesDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures");
const readFixture = (name: string) => fs.readFileSync(path.join(fixturesDir, name), "utf8");
const manifest = JSON.parse(
  fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.json"), "utf8"),
);

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

const manifestSeed = {
  models: [
    { id: "auto", displayName: "Auto", description: "Manifest fallback" },
  ],
};

function loadCursorWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const baseHost = (over: Record<string, unknown>) => ({
  manifestJson: () => JSON.stringify(manifestSeed),
  homeDir: () => "/tmp/home",
  platform: () => "linux",
  expandHome: (v: string) => v.replace(/^~/, "/tmp/home"),
  envGet: () => null,
  unixNowMs: () => 0,
  md5: (s: string) => crypto.createHash("md5").update(s).digest("hex"),
  exec: () => JSON.stringify({ code: 127, error: "not found" }),
  log: () => {},
  shell: {
    quoteFor: (v: unknown) => String(v),
    quote: (v: unknown) => String(v),
    envPrefix: (n: unknown, v: unknown, platform: unknown) =>
      platform === "windows" ? `$env:${n}='${v}'; ` : `${n}='${v}' `,
  },
  fs: {
    fileExists: () => false,
    readFile: () => null,
    readJson: () => null,
    writeJson: () => true,
    writeFile: () => true,
    readDir: () => [],
    walkDir: () => [],
    sqliteQuery: () => [],
  },
  ...over,
});

console.log("\n--- meta ---\n");
eq(plugin.meta.id, "cursor", "meta.id is 'cursor'");
eq(plugin.meta.displayName, "Cursor", "meta.displayName");
eq(manifest.hostApi, 2, "manifest declares hostApi 2");

console.log("\n--- detect (frontend, structural fingerprints) ---\n");

// Captured from a live cursor-agent TUI (rendered screen).
const cursorFixture = [
  "  Cursor Agent",
  "  v2026.06.16-20-30-07-a07d3ac",
  "  Type ? in the prompt bar to show in-app hints.",
  "  → Plan, search, build anything",
  "  Auto                                                  Run Everything",
  "  /private/tmp/ct_cursor_probe",
].join("\n");

// Captured trust gate.
const cursorTrustFixture = [
  "  ⚠ Workspace Trust Required",
  "  Cursor Agent can execute code and access files in this directory.",
  "  ▶ [a] Trust this workspace",
].join("\n");

assert(plugin.detect({ bytes: "", title: "cursor" }), "title 'cursor' positive");
assert(plugin.detect({ bytes: "", title: "Cursor Agent" }), "title 'Cursor Agent' (case-insensitive)");
assert(plugin.detect({ bytes: cursorFixture }), "real banner output detects (multi-signal)");
assert(plugin.detect({ bytes: cursorTrustFixture }), "real trust-gate output detects");
for (const literal of ["Type ? in the prompt bar", "Run Everything", "Plan, search, build anything"]) {
  assert(
    plugin.detect({ bytes: cursorFixture.replace(literal, "[changed copy]") }),
    `single literal mutation still detects: ${literal}`,
  );
}

console.log("\n--- detect negatives ---\n");
assert(!plugin.detect({ bytes: "$ ls\nfile.txt", title: "bash" }), "plain shell negative");
assert(!plugin.detect({ bytes: "OpenAI Codex\ncodex/gpt-5\nctrl+c to cancel", title: "codex" }), "codex sample is not cursor");
assert(!plugin.detect({ bytes: "Running opencode v1.2\nOPENCODE banner", title: "opencode" }), "opencode sample is not cursor");
assert(!plugin.detect({ bytes: "claude code", title: "claude" }), "claude sample is not cursor");
assert(!plugin.detect({ bytes: "I opened the Cursor Agent docs today" }), "stray 'Cursor Agent' mention alone is not enough");

console.log("\n--- statusOf / actions / patterns ---\n");
eq(plugin.statusOf({ kind: "anything" }), null, "statusOf always returns null (PTY-driven status)");
eq(plugin.actions, [], "no actions defined yet");
assert(plugin.titlePattern instanceof RegExp, "titlePattern is RegExp");
assert(plugin.titlePattern!.test("cursor"), "titlePattern matches 'cursor'");
assert(Array.isArray(plugin.noisePatterns) && plugin.noisePatterns.length > 0, "noisePatterns non-empty");
assert(plugin.imageSend === undefined, "imageSend undefined (host falls back to path strategy)");

console.log("\n--- runtime detect (plugin.js, structural) ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  assert(p.detectFromTitle("Cursor 1.0") === true, "detectFromTitle positive");
  assert(p.detectFromTitle("bash") === false, "detectFromTitle negative");
  assert(p.detectFromOutput(cursorTrustFixture) === true, "detectFromOutput trust gate positive");
  assert(p.detectFromOutput(cursorFixture) === true, "detectFromOutput banner positive (two-signal)");
  assert(p.detectFromOutput("OpenAI Codex\ncodex>") === false, "detectFromOutput codex negative");
  assert(p.screenHasTui("foo\n  Run Everything\nbar") === true, "screenHasTui positive");
  assert(p.screenHasTui(cursorTrustFixture) === false, "workspace trust gate is not a ready agent TUI");
  assert(p.screenHasTui("$ ls") === false, "screenHasTui negative");
  assert(p.isOutputNoise("╭──────────╮") === true, "box-drawing line is noise");
  assert(p.isOutputNoise("hello") === false, "text line is not noise");
}

console.log("\n--- buildLaunchCommand (workspace-trust pre-write + invocation) ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  // gate. Pre-writing it unconditionally would silently bypass that gate.
  const bare = p.buildLaunchCommand({});
  assert(bare.indexOf(".workspace-trusted") === -1, "bare launch does NOT pre-write the trust marker (gate honored)");
  eq(
    bare,
    "CLAUDE_CODE_DISABLE_HOOKS='1' cursor-agent",
    "bare launch is the hook-disable prefix + invocation, no trust prefix",
  );
  // With skipPermissions (--force, the user's intentional 'trust everything'
  // flow): pre-write the marker so the TUI doesn't block on it.
  const forced = p.buildLaunchCommand({ skipPermissions: true });
  assert(forced.indexOf(".workspace-trusted") !== -1, "skipPermissions pre-writes the trust marker");
  assert(forced.endsWith("cursor-agent --force"), "skipPermissions adds --force after the trust prefix");
  const withModel = p.buildLaunchCommand({ skipPermissions: true, args: ["--model", "auto"] });
  assert(withModel.endsWith("cursor-agent --force --model auto"), "force + model args applied");
  assert(withModel.indexOf("pwd -P") !== -1, "POSIX trust prefix resolves symlinks via pwd -P");
  const hostileTask = "line one\n<location>D:\\repo</location>\n'quotes' \"double\" $HOME `whoami` --flag\nюникод";
  const taskProvider = loadCursorWithHost(baseHost({
    shell: {
      quoteFor: (value: unknown) => JSON.stringify(String(value)),
      quote: (value: unknown) => JSON.stringify(String(value)),
      envPrefix: (name: unknown, value: unknown) => `${name}='${value}' `,
    },
  }));
  const withTask = taskProvider.buildLaunchCommand({ skipPermissions: true, task: hostileTask });
  assert(
    withTask.endsWith(`cursor-agent --force ${JSON.stringify(hostileTask)}`),
    "Cursor initial task is one shell-quoted positional prompt",
  );
}

console.log("\n--- POSIX trustPrefix: JSON-escaped workspacePath ---\n");
{
  const p = loadCursorWithHost(baseHost({ platform: () => "linux" }));
  const cmd = p.buildLaunchCommand({ skipPermissions: true });
  // The sed JSON-escape step must be present (escapes \ then ").
  assert(cmd.indexOf("sed 's/") !== -1, "POSIX trust prefix includes sed for JSON-escaping the path");
  // The printf JSON output must use the escaped variable $__ctdj, not the raw $__ctd.
  const printfIdx = cmd.indexOf("printf '{");
  assert(printfIdx !== -1, "printf JSON line is present");
  const printfPart = cmd.substring(printfIdx);
  assert(printfPart.indexOf("\"$__ctdj\"") !== -1, "printf JSON uses JSON-escaped $__ctdj");
  // Raw $__ctd (with closing quote, not $__ctdj) must NOT appear in the JSON printf.
  // ($__ctdj itself contains $__ctd as a substring so check for the closing quote boundary.)
  assert(printfPart.indexOf("\"$__ctd\"") === -1, "raw $__ctd not used in printf JSON (would break on quotes/backslashes in path)");
}

console.log("\n--- buildResumeCommand (trust pre-write GATED behind skipPermissions) ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  const r = p.buildResumeCommand("1a7e8655-f97a-0000-0000-000000000000");
  assert(r.indexOf(".workspace-trusted") === -1, "resume without skipPermissions does NOT pre-write the trust marker");
  assert(
    r === "CLAUDE_CODE_DISABLE_HOOKS='1' cursor-agent --resume 1a7e8655-f97a-0000-0000-000000000000",
    "resume by session uuid carries the hook-disable prefix, no trust prefix",
  );
  const forced = p.buildResumeCommand("abc", true);
  assert(forced.indexOf(".workspace-trusted") !== -1, "resume with skipPermissions pre-writes the trust marker");
  assert(forced.endsWith("cursor-agent --resume abc --force"), "resume carries --force when skipPermissions");
}

console.log("\n--- Claude PreToolUse hook import is disabled for every Cursor launch (sec_hook_ampersand) ---\n");
{
  // Regression: cursor-agent's Bash tool picks up ~/.claude/settings.json's
  // PreToolUse hooks. A hook authored for Claude's shell dialect (e.g.
  // PowerShell body) crashes Cursor's POSIX bash runner with a bare `&`
  // parse error. CLAUDE_CODE_DISABLE_HOOKS=1 must be set on every launch,
  // on both platforms, without ever touching the user's settings file.
  const posix = loadCursorWithHost(baseHost({ platform: () => "linux" }));
  for (const cmd of [
    posix.buildLaunchCommand({}),
    posix.buildLaunchCommand({ skipPermissions: true }),
    posix.buildResumeCommand("abc"),
    posix.buildResumeCommand("abc", true),
  ]) {
    assert(cmd.indexOf("CLAUDE_CODE_DISABLE_HOOKS='1'") === 0, "POSIX: hook-disable is the leading prefix", cmd);
    assert(!cmd.includes("&&"), "POSIX: hook-disable prefix needs no `&&` chaining", cmd);
  }

  const win = loadCursorWithHost(baseHost({ platform: () => "windows" }));
  for (const cmd of [
    win.buildLaunchCommand({}),
    win.buildLaunchCommand({ skipPermissions: true }),
    win.buildResumeCommand("abc"),
    win.buildResumeCommand("abc", true),
  ]) {
    assert(cmd.indexOf("$env:CLAUDE_CODE_DISABLE_HOOKS='1'; ") === 0, "windows: hook-disable is the leading prefix", cmd);
  }
}

console.log("\n--- buildLaunchCommand (windows trust prefix, gated) ---\n");
{
  const p = loadCursorWithHost(baseHost({ platform: () => "windows" }));
  const bare = p.buildLaunchCommand({});
  assert(bare.indexOf(".workspace-trusted") === -1, "windows bare launch does NOT pre-write the trust marker");
  assert(
    bare === "$env:CLAUDE_CODE_DISABLE_HOOKS='1'; cursor-agent",
    "windows bare launch is the hook-disable prefix + invocation",
  );
  const cmd = p.buildLaunchCommand({ skipPermissions: true });
  assert(cmd.indexOf(".workspace-trusted") !== -1, "windows launch with skipPermissions pre-writes the trust marker");
  assert(cmd.indexOf("Get-Location") !== -1, "windows trust prefix uses PowerShell Get-Location");
  assert(cmd.indexOf("[^a-zA-Z0-9]+") !== -1, "windows trust slug collapses adjacent drive separators");
  assert(cmd.indexOf("ConvertTo-Json -Compress") !== -1, "windows trust marker uses JSON serialization");
  assert(cmd.indexOf("[IO.File]::WriteAllText") !== -1, "windows trust marker is replaced atomically before launch");
  assert(cmd.indexOf("workspacePath\\\":\\\"'+($d -replace") === -1, "windows trust path is not escaped twice");
  assert(cmd.endsWith("cursor-agent --force"), "windows launch ends with the invocation");
}

console.log("\n--- cursorChatsDir md5 keys off the NATIVE-separator cwd (ROG md5 fix) ---\n");
{
  let winSeen: string | null = null;
  const pWin = loadCursorWithHost(baseHost({
    platform: () => "windows",
    md5: (s: string) => { winSeen = s; return "HASH"; },
  }));
  pWin.sessionExists("C:/Users/u/proj", "1a7e8655-f97a-0000-0000-000000000000");
  eq(winSeen, "C:\\Users\\u\\proj", "windows: md5 input has native backslashes, not forward slashes");

  let nixSeen: string | null = null;
  const pNix = loadCursorWithHost(baseHost({
    platform: () => "linux",
    md5: (s: string) => { nixSeen = s; return "HASH"; },
  }));
  pNix.sessionExists("/Users/u/proj", "1a7e8655-f97a-0000-0000-000000000000");
  eq(nixSeen, "/Users/u/proj", "posix: md5 input unchanged (forward slashes already native)");
}

console.log("\n--- detectSessionId (identity-bound: refuse when ambiguous) ---\n");
{
  const sep = "/";
  const chatsDir = "/home/u/.cursor/chats/HASH";
  const s1 = "11111111-1111-4111-8111-111111111111";
  const s2 = "22222222-2222-4222-8222-222222222222";
  // Two viable sibling sessions under the cwd hash, both recent.
  const p = loadCursorWithHost(baseHost({
    homeDir: () => "/home/u",
    md5: () => "HASH",
    unixNowMs: () => 1000000,
    fs: {
      ...baseHost({}).fs,
      fileExists: (pth: string) => pth === chatsDir || pth.indexOf("meta.json") !== -1,
      readDir: (pth: string) => pth === chatsDir
        ? [
            { name: s1, path: chatsDir + sep + s1, isDir: true, isFile: false },
            { name: s2, path: chatsDir + sep + s2, isDir: true, isFile: false },
          ]
        : [],
      readJson: () => ({ updatedAtMs: 999999 }),
    },
  }));
  eq(p.detectSessionId("/proj"), null, "2+ viable candidates → refuse (null), never newest-for-cwd");
  // Excluding the sibling collapses the set to one → that unique candidate binds.
  eq(p.detectSessionId("/proj", s2), s1, "with the sibling excluded exactly one candidate → bind it");
  eq(p.detectSessionId("/proj", [s1, s2]), null, "all excluded → no candidate → null");
}

console.log("\n--- detectSessionId: empty UUID dir alongside real session binds the real one ---\n");
{
  const sep = "/";
  const chatsDir = "/home/u/.cursor/chats/HASH";
  const s1 = "11111111-1111-4111-8111-111111111111"; // has meta.json → real session
  const s2 = "22222222-2222-4222-8222-222222222222"; // empty dir (no meta.json, no store.db)
  const p = loadCursorWithHost(baseHost({
    homeDir: () => "/home/u",
    md5: () => "HASH",
    unixNowMs: () => 1000000,
    fs: {
      ...baseHost({}).fs,
      fileExists: (pth: string) => {
        if (pth === chatsDir) return true;
        if (pth === chatsDir + sep + s1 + sep + "meta.json") return true;
        return false;
      },
      readDir: (pth: string) => pth === chatsDir
        ? [
            { name: s1, path: chatsDir + sep + s1, isDir: true, isFile: false },
            { name: s2, path: chatsDir + sep + s2, isDir: true, isFile: false },
          ]
        : [],
      readJson: () => ({ updatedAtMs: 999999 }),
    },
  }));
  eq(p.detectSessionId("/proj"), s1, "empty UUID dir + real session → bind the real one (not refused)");
  eq(p.detectSessionId("/proj", s1), null, "real session excluded → empty dir not a candidate → null");
}

console.log("\n--- discoverModels (live --list-models, manifest fallback) ---\n");
{
  // No CLI available → host.exec returns an error envelope → manifest seed.
  const seedHost = loadCursorWithHost(baseHost({ exec: () => JSON.stringify({ code: 127, error: "not found" }) }));
  eq(seedHost.discoverModels(), manifestSeed.models, "discoverModels falls back to the manifest seed when CLI absent");

  // status annotations stripped, header/tip lines ignored.
  const listOutput = [
    "Available models",
    "",
    "auto - Auto (current)",
    "composer-2.5 - Composer 2.5",
    "composer-2.5-fast - Composer 2.5 Fast (default)",
    "claude-4.6-sonnet-medium - Sonnet 4.6 1M",
    "claude-opus-4-8-thinking-high - Opus 4.8 1M Thinking",
    "gpt-5.5-high - GPT-5.5 1M High",
    "gemini-3.1-pro - Gemini 3.1 Pro",
    "claude-fable-5-low - Fable 5 1M Low (NO ZDR)",
    "",
    "Tip: use --model <id> (or /model <id> in interactive mode) to switch.",
  ].join("\n");
  const liveHost = loadCursorWithHost(baseHost({ exec: () => JSON.stringify({ code: 0, stdout: listOutput }) }));
  const models = liveHost.discoverModels();
  const byId: Record<string, string> = {};
  for (const m of models) byId[m.id] = m.displayName;
  eq(byId["auto"], "Auto", "auto: (current) annotation stripped");
  eq(byId["composer-2.5-fast"], "Composer 2.5 Fast", "(default) annotation stripped");
  eq(byId["claude-opus-4-8-thinking-high"], "Opus 4.8 1M Thinking", "real reconciled id parsed");
  eq(byId["claude-fable-5-low"], "Fable 5 1M Low (NO ZDR)", "informative (NO ZDR) annotation kept");
  assert(byId["Available models"] === undefined, "header line ignored");
  assert(models.every((m: { id: string }) => m.id.indexOf("Tip") === -1), "tip footer ignored");
}

// is stripped from the label, and embedding ids are filtered out.
{
  const listOutput = [
    "auto - Auto (current)",
    "claude-opus-4-8 - Opus 4.8",
    "gpt-5.5-pro - GPT-5.5 Pro (requires Pro)",
    "text-embedding-3-large - Embeddings",
  ].join("\n");
  const liveHost = loadCursorWithHost(baseHost({ exec: () => JSON.stringify({ code: 0, stdout: listOutput }) }));
  const models = liveHost.discoverModels() as Array<{ id: string; displayName: string; available?: boolean }>;
  const byId: Record<string, { displayName: string; available?: boolean }> = {};
  for (const m of models) byId[m.id] = m;
  assert(byId["text-embedding-3-large"] === undefined, "embedding model filtered out");
  eq(byId["auto"].available, undefined, "available model leaves availability unset");
  eq(byId["gpt-5.5-pro"].available, false, "(requires Pro) → available:false");
  eq(byId["gpt-5.5-pro"].displayName, "GPT-5.5 Pro", "lockout annotation stripped from label");
}

console.log("\n--- projectHash + slug (md5 of canonical cwd) ---\n");
{
  const p = loadCursorWithHost(baseHost({
    homeDir: () => "/home/u",
    md5: (s: string) => crypto.createHash("md5").update(s).digest("hex"),
    fs: { ...baseHost({}).fs, fileExists: () => false, readDir: () => [] },
  }));
  // Grounded fact: cursor keys the chats dir by md5(realpath(cwd)).
  eq(
    crypto.createHash("md5").update("/private/tmp/ct_cursor_probe").digest("hex"),
    "99e0288bea5c101a380aaa11fe4cf4a0",
    "md5(/private/tmp/ct_cursor_probe) matches the on-disk chats hash",
  );
  // sessionExists derives ~/.cursor/chats/<md5>/<sid> and probes meta.json/store.db.
  let probed = "";
  const eh = loadCursorWithHost(baseHost({
    homeDir: () => "/home/u",
    md5: (s: string) => crypto.createHash("md5").update(s).digest("hex"),
    fs: { ...baseHost({}).fs, fileExists: (pth: string) => { probed = pth; return pth.indexOf("meta.json") !== -1; } },
  }));
  assert(eh.sessionExists("/private/tmp/ct_cursor_probe", "1a7e8655-f97a-490f-bfc6-74552c4b2c98") === true, "sessionExists true when meta.json present");
  assert(
    probed.indexOf("/home/u/.cursor/chats/99e0288bea5c101a380aaa11fe4cf4a0/1a7e8655-f97a-490f-bfc6-74552c4b2c98") !== -1,
    "sessionExists probes the md5-keyed chats dir",
  );
}

console.log("\n--- parseSessionDelta (real JSONL transcript fixture) ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  const delta = p.parseSessionDelta(readFixture("transcript.jsonl")) as { messages: any[] };
  eq(delta.messages.length, 3, "3 message rows (turn_ended skipped)");
  // 1) user turn: <user_query> wrapper stripped.
  eq(delta.messages[0].role, "user", "row 0 is user");
  eq(delta.messages[0].blocks[0].kind, "text", "user block is text");
  eq(delta.messages[0].blocks[0].data.text, "Run the shell command 'echo hello123' and tell me what it printed.", "user_query framing stripped");

  const timestamped = p.parseSessionDelta(JSON.stringify({
    role: "user",
    message: {
      content: [{
        type: "text",
        text: "<timestamp>Monday, Jul 20, 2026, 1:21 PM (UTC+3)</timestamp>\n<user_query>\nCompute 5096 + 3837. Reply with the decimal digits only.\n</user_query>",
      }],
    },
  })) as { messages: any[] };
  eq(
    timestamped.messages[0].blocks[0].data.text,
    "Compute 5096 + 3837. Reply with the decimal digits only.",
    "timestamp and user_query framing stripped together",
  );
  // 2) assistant turn with a redacted-thinking text block + a tool_use block:
  //    the bare [REDACTED] text block is dropped, tool_use survives.
  eq(delta.messages[1].role, "assistant", "row 1 is assistant");
  eq(delta.messages[1].blocks.length, 1, "redacted-only text block dropped, tool_use kept");
  eq(delta.messages[1].blocks[0].kind, "tool_use", "tool_use block preserved");
  eq(delta.messages[1].blocks[0].data.name, "Shell", "tool name preserved");
  eq(delta.messages[1].blocks[0].data.input.command, "echo hello123", "tool input preserved");
  eq(delta.messages[1].blocks[0].data.tool_kind, "command", "Shell gets canonical command kind");
  eq(delta.messages[1].blocks[0].data.tool_summary, "echo hello123", "Shell gets clean command summary");
  eq(delta.messages[1].blocks[0].data.tool_detail, "echo hello123", "Shell gets command detail");
  eq(delta.messages[1].blocks[0].data.tool_edits, [], "Shell has no edit payload");
  eq(delta.messages[1].blocks[0].data.tool_status, "success", "persisted Cursor tool use is complete");
  // 3) assistant final text: trailing [REDACTED] trimmed, body kept.
  eq(delta.messages[2].blocks[0].kind, "text", "final assistant block is text");
  eq(delta.messages[2].blocks[0].data.text, "The command printed:\n\n```\nhello123\n```", "trailing [REDACTED] stripped");

  // parseSessionMessages mirrors it for content search (text only, no tool_use).
  const msgs = p.parseSessionMessages(readFixture("transcript.jsonl")) as any[];
  eq(msgs.length, 2, "content search yields the 2 text-bearing turns");
  eq(msgs[0].text, "Run the shell command 'echo hello123' and tell me what it printed.", "search text matches user turn");
  assert(msgs[1].text.indexOf("hello123") !== -1, "search text matches assistant turn");
}

console.log("\n--- canonical tool fields ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  const parsed = p.parseSessionDelta(JSON.stringify({ role: "assistant", message: { content: [
    { type: "tool_use", name: "Edit", input: { file_path: "src/demo.ts", old_string: "old", new_string: "new" } },
    { type: "tool_use", name: "Mystery", input: { value: "opaque" } },
  ] } })) as { messages: Array<{ blocks: Array<{ data: Record<string, unknown> }> }> };
  const data = parsed.messages[0].blocks.map((block) => block.data);
  eq(data[0].tool_kind, "edit", "Edit gets canonical edit kind");
  eq(data[0].tool_edits, [{ path: "src/demo.ts", old: "old", new: "new" }], "Edit emits canonical edit pair");
  eq(data[1].tool_kind, "generic", "unknown tool degrades to generic");
  assert(typeof data[1].tool_summary === "string" && !(data[1].tool_summary as string).includes("{"), "unknown summary is not JSON");
}

console.log("\n--- detectSessionModel (store.db JSON blob scan) ---\n");
{
  const p = loadCursorWithHost(baseHost({
    homeDir: () => "/home/u",
    md5: (s: string) => crypto.createHash("md5").update(s).digest("hex"),
    fs: {
      ...baseHost({}).fs,
      fileExists: (pth: string) => pth.indexOf("store.db") !== -1,
      sqliteQuery: () => [
        { t: '{"id":"1","role":"assistant","modelName":"composer-2.5"}' },
        { t: null },
      ],
    },
  }));
  eq(p.detectSessionModel("/private/tmp/ct_cw2_probe", "cbf7919a-ce25-423d-8e18-1aaae2d82701"), "composer-2.5", "modelName extracted from JSON blob");
}

console.log("\n--- enumerateSessions (walk ~/.cursor/projects agent-transcripts) ---\n");
{
  const sid = "cbf7919a-ce25-423d-8e18-1aaae2d82701";
  const base = `C:\\Users\\u\\.cursor\\projects\\D-Developer-codeterm\\agent-transcripts\\${sid}`;
  const p = loadCursorWithHost(baseHost({
    homeDir: () => "C:\\Users\\u",
    platform: () => "windows",
    fs: {
      ...baseHost({}).fs,
      fileExists: (value: string) => value === "D:\\Developer\\codeterm" || value.indexOf(".cursor") !== -1,
      walkDir: () => [
        { name: `${sid}.jsonl`, path: `${base}\\${sid}.jsonl`, isFile: true, isDir: false, size: 523, createdMs: 1781885008048, modifiedMs: 1781885033657 },
        { name: "repo.json", path: "C:\\Users\\u\\.cursor\\projects\\D-Developer-codeterm\\repo.json", isFile: true, isDir: false, size: 50, createdMs: null, modifiedMs: null },
      ],
    },
  }));
  const sessions = p.enumerateSessions() as any[];
  eq(sessions.length, 1, "only the agent-transcript jsonl enumerated (repo.json ignored)");
  eq(sessions[0].sessionId, sid, "session id is the transcript uuid");
  eq(sessions[0].provider, "cursor", "provider tagged cursor");
  eq(sessions[0].projectPath, "D:\\Developer\\codeterm", "projectPath decoded and verified against the filesystem");
  eq(sessions[0].createdTs, Math.floor(1781885008048 / 1000), "createdTs from file stat (unix seconds)");
}

console.log("\n--- chat path: JSONL tailer, not structured ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  eq(p.usesStructuredChat(), false, "usesStructuredChat false → host uses the JSONL byte-offset tailer");
  eq(p.parseUsage("any tui screen text", 0), null, "parseUsage null (cursor TUI has no usage gauge)");
}

console.log("\n--- parsePrompt + hasEventMarkers (interactive menu extraction) ---\n");
{
  const p = loadCursorWithHost(baseHost({}));

  // Workspace-trust style numbered menu inside a rounded box.
  const trust = [
    "╭───────────────────────────────────────────╮",
    "│  Do you trust the files in this folder?     │",
    "│                                             │",
    "│  ❯ 1. Yes, allow this workspace             │",
    "│    2. No, exit                              │",
    "│                                             │",
    "│  Enter to confirm · Esc to cancel           │",
    "╰───────────────────────────────────────────╯",
  ].join("\n");
  const parsed = p.parsePrompt(trust);
  eq(parsed && parsed.question, "Do you trust the files in this folder?", "cursor parsePrompt extracts the question");
  eq(
    parsed && parsed.options,
    [["Yes, allow this workspace", "1"], ["No, exit", "2"]],
    "cursor parsePrompt extracts ALL options (selector glyph stripped)",
  );
  assert(p.hasEventMarkers("  ❯ 1. Yes"), "hasEventMarkers fires on a selector glyph chunk");
  assert(p.hasEventMarkers("Workspace Trust Required"), "hasEventMarkers fires on trust keyword");
  assert(!p.hasEventMarkers("regular streaming assistant text"), "hasEventMarkers stays quiet on normal output");
  eq(p.detectEvents(trust), ["permission_request"], "detectEvents surfaces permission_request for a real menu");

  // Three-option command-approval menu (footer-gated, no selector glyph).
  const approve = [
    "Allow this command to run?",
    "  1. Yes, once",
    "  2. Yes, always",
    "  3. No",
    "Press Enter to confirm · Esc to cancel",
  ].join("\n");
  const a = p.parsePrompt(approve);
  eq(
    a && a.options,
    [["Yes, once", "1"], ["Yes, always", "2"], ["No", "3"]],
    "footer-gated numbered menu → all 3 options",
  );

  // Normal assistant prose with a numbered list must NOT surface a card.
  const prose = "Here is the plan:\n1. Refactor parser\n2. Add tests\n3. Ship it\n\nLet me start.";
  eq(p.parsePrompt(prose), null, "numbered prose without selector/footer → null (no phantom prompt)");
  eq(p.detectEvents(prose), [], "no event for numbered prose");
}

console.log("\n--- parsePrompt: /model non-numbered list picker (live fixture) ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  // with a → selector on the highlight and a "Enter to select / Type to filter" footer.
  const modelScreen = readFixture("model-picker.txt");
  const m = p.parsePrompt(modelScreen);
  assert(m !== null, "cursor /model list picker is parsed (was null before P9.4)");
  eq(m && m.question, "Available models", "question is the picker title");
  assert(m && m.options.length >= 2, `>=2 options extracted (got ${m ? m.options.length : 0})`);
  eq(m && m.options[0], ["Composer 2.5", "1"], "highlighted (→) row is first option, metadata stripped");
  assert(
    !!(m && m.options.some((o: [string, string]) => o[0] === "Opus 4.8")),
    "trailing '300K High' metadata dropped from the label",
  );
  assert(
    !!(m && m.options.every((o: [string, string]) => o[0].indexOf("of 28") === -1 && o[0].indexOf("filter") === -1)),
    "pagination + footer rows are not mistaken for options",
  );
  assert(p.hasEventMarkers("Type to filter • Enter to select • Tab to edit"), "hasEventMarkers fires on the /model footer");
  assert(p.hasEventMarkers(" →  Composer 2.5"), "hasEventMarkers fires on the → selector row");
  eq(p.detectEvents(modelScreen), ["permission_request"], "detectEvents surfaces the /model picker");
  // A bare non-numbered list with no selector and no picker footer must NOT card.
  const bareList = "Auto\nComposer 2.5\nOpus 4.8\nGPT-5.5\nthat's the lineup.";
  eq(p.parsePrompt(bareList), null, "non-numbered list without selector/footer → null (no phantom card)");
}

console.log("\n--- parsePrompt: cursor rule/skills selectors ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  const grouped = [
    " Create New Rule",
    "",
    " When should this rule be applied?",
    "",
    "  → Always Apply",
    "      Apply to every chat session",
    "    Apply Intelligently",
    "      Agent decides based on description",
    "    Apply to Specific Files",
    "      When file matches a pattern",
    "    Apply Manually",
    "      Only when @-mentioned in chat",
    "",
    " ↑/↓ to navigate • Enter to confirm • Esc to cancel",
  ].join("\n");
  const g = p.parsePrompt(grouped);
  eq(g && g.question, "When should this rule be applied?", "grouped selector question extracted");
  eq(
    g && g.options,
    [["Always Apply", "1"], ["Apply Intelligently", "2"], ["Apply to Specific Files", "3"], ["Apply Manually", "4"]],
    "grouped selector extracts labels and skips descriptions",
  );
  eq(p.buildPromptResponse(0, grouped), ["DQ=="], "grouped selector highlighted row click → Enter only");
  eq(p.buildPromptResponse(2, grouped), ["G1tC", "G1tC", "DQ=="], "grouped selector click below highlight → down ×2 + Enter");

  const single = [
    " Rules (0 rules)",
    "",
    "  → + Create new rule",
    "",
    " ↑/↓ to navigate • Enter to select • d to delete • Esc to close",
  ].join("\n");
  const s = p.parsePrompt(single);
  eq(s && s.question, "Rules (0 rules)", "single-row selector title extracted");
  eq(s && s.options, [["+ Create new rule", "1"]], "single highlighted row is a valid picker");
  eq(p.buildPromptResponse(0, single), ["DQ=="], "single-row click sends Enter");
}

console.log("\n--- buildPromptResponse (vertical ↓ nav + Enter) ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  // No screen → top-start fallback (highlight unknown → assume index 0).
  eq(p.buildPromptResponse(0), ["DQ=="], "index 0, no screen → Enter only");
  eq(p.buildPromptResponse(2), ["G1tC", "G1tC", "DQ=="], "index 2, no screen → down ×2 + Enter (ESC[B)");

  // NOT the top row. Here Auto=0, Composer 2.5 (→ highlighted)=1, Opus 4.8=2, GPT-5.5=3.
  const screen = [
    " Available models                                          Max mode: OFF",
    " Filter:",
    "    Auto",
    " →  Composer 2.5             (Tab to modify)",
    "    Opus 4.8                 300K High",
    "    GPT-5.5                  272K Medium",
    " 1-10 of 28",
    " Type to filter • Enter to select • Tab to edit",
  ].join("\n");
  // sanity: the highlight sits at index 1, not the top → naive down×N would mis-pick.
  eq(p.parsePrompt(screen).options[1], ["Composer 2.5", "2"], "fixture: highlighted row is index 1 (Composer 2.5)");
  eq(p.buildPromptResponse(1, screen), ["DQ=="], "target == current highlight → Enter only (no drift)");
  eq(p.buildPromptResponse(2, screen), ["G1tC", "DQ=="], "target below highlight → 1 down + Enter");
  eq(p.buildPromptResponse(3, screen), ["G1tC", "G1tC", "DQ=="], "target 2 below → 2 down + Enter");
  eq(p.buildPromptResponse(0, screen), ["G1tB", "DQ=="], "target above highlight → 1 up + Enter (ESC[A)");
  // A screen with no detectable highlight → top-start fallback (down × target).
  const noHighlight = "some assistant prose with no menu at all";
  eq(p.buildPromptResponse(2, noHighlight), ["G1tC", "G1tC", "DQ=="], "no highlight in screen → top-start fallback");
}

{
  const p = loadCursorWithHost(
    baseHost({
      platform: () => "windows",
      shell: {
        quoteFor: (v: unknown, platform?: unknown) => `Q(${platform}:${String(v)})`,
        quote: (v: unknown) => `BAD(${String(v)})`,
        envPrefix: (n: unknown, v: unknown) => `ENV(${n}=${v}) `,
      },
    }),
  );
  eq(
    p.buildResumeCommand("sess 1"),
    "ENV(CLAUDE_CODE_DISABLE_HOOKS=1) cursor-agent --resume Q(windows:sess 1)",
    "buildResumeCommand quotes session id with quoteFor(platform)",
  );
}

console.log("\n--- usage (dashboard API fetch + parse) ---\n");
{
  // Real GetCurrentPeriodUsage response, captured live. planUsage values are CENTS.
  const currentFixture = readFixture("usage-current-period.json");
  const RESETS_AT = 1784473493000; // billingCycleEnd
  const PERIOD_START = 1781881493000; // billingCycleStart
  const SPEND_CENTS = 18856; // totalSpend, verbatim cents (NOT 1885600)
  const LIMIT_CENTS = 2000; // limit as cents
  const REMAINING_CENTS = 0; // max(0, limit - includedSpend) = max(0, 2000 - 2000)
  const WEEKLY_PCT = 96.6974; // totalPercentUsed verbatim
  const SESSION_PCT = 84.8444; // apiPercentUsed verbatim
  const nowMs = 1781000000000;

  const usageHost = (over: Record<string, unknown> = {}) =>
    baseHost({
      platform: () => "macos",
      exec: () => JSON.stringify({ code: 0, stdout: "cursor-test-token\n" }),
      fetch: (optsJson: string) => {
        const url = String(JSON.parse(optsJson).url || "");
        if (url.indexOf("GetCurrentPeriodUsage") !== -1) return JSON.stringify({ status: 200, body: currentFixture });
        return JSON.stringify({ status: 404, body: "{}" });
      },
      ...over,
    });

  const p = loadCursorWithHost(usageHost());
  const raw = p.fetchUsage(nowMs);
  assert(typeof raw === "string", "fetchUsage returns a raw JSON string");
  const payload = JSON.parse(String(raw));
  eq(payload.spendCents, SPEND_CENTS, "fetchUsage spend = planUsage.totalSpend (cents, no x100)");
  eq(payload.limitCents, LIMIT_CENTS, "fetchUsage limit = planUsage.limit (cents)");
  eq(payload.remainingCents, REMAINING_CENTS, "fetchUsage remaining = max(0, limit - includedSpend)");
  eq(payload.totalPct, WEEKLY_PCT, "fetchUsage totalPct = planUsage.totalPercentUsed verbatim");
  eq(payload.apiPct, SESSION_PCT, "fetchUsage apiPct = planUsage.apiPercentUsed verbatim");
  eq(payload.autoPct, 100, "fetchUsage autoPct = planUsage.autoPercentUsed verbatim");
  eq(payload.resetsAtMs, RESETS_AT, "fetchUsage reset = billingCycleEnd");
  eq(payload.periodStartMs, PERIOD_START, "fetchUsage periodStart = billingCycleStart");

  const snap = p.parseUsage(String(raw), nowMs) as Record<string, unknown>;
  eq(snap.provider, "cursor", "parseUsage provider is cursor");
  eq(snap.weekly_pct, WEEKLY_PCT, "parseUsage weekly_pct = totalPercentUsed verbatim");
  eq(snap.session_pct, SESSION_PCT, "parseUsage session_pct = apiPercentUsed verbatim");
  eq(snap.weekly_resets_at_ms, RESETS_AT, "parseUsage weekly_resets_at_ms = billingCycleEnd");
  eq(snap.session_resets_at_ms, RESETS_AT, "parseUsage session_resets_at_ms = billingCycleEnd");
  eq(snap.spend_cents, SPEND_CENTS, "parseUsage spend_cents verbatim cents (18856, not 1885600)");
  eq(snap.spend_limit_cents, LIMIT_CENTS, "parseUsage spend_limit_cents = limit as cents");
  eq(snap.spend_remaining_cents, REMAINING_CENTS, "parseUsage spend_remaining_cents = max(0, limit - includedSpend)");
  eq(snap.spend_resets_at_ms, RESETS_AT, "parseUsage spend_resets_at_ms = billingCycleEnd");

  // No token → no usage, honestly (not a fabricated zero).
  const noTokenHost = usageHost({ exec: () => JSON.stringify({ code: 127, error: "not found" }) });
  eq(loadCursorWithHost(noTokenHost).fetchUsage(nowMs), null, "fetchUsage null when keychain token missing");

  // Windows has no clean keychain CLI read → null (usage hidden, not broken).
  const winHost = usageHost({ platform: () => "windows" });
  eq(loadCursorWithHost(winHost).fetchUsage(nowMs), null, "fetchUsage null on windows (no token path)");

  // Garbage payload never yields a bogus snapshot.
  eq(p.parseUsage("not json", nowMs), null, "parseUsage null on non-JSON");
  eq(p.parseUsage(JSON.stringify({ nope: 1 }), nowMs), null, "parseUsage null without spendCents");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
