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
  shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
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
  assert(p.screenHasTui("$ ls") === false, "screenHasTui negative");
  assert(p.isOutputNoise("╭──────────╮") === true, "box-drawing line is noise");
  assert(p.isOutputNoise("hello") === false, "text line is not noise");
}

console.log("\n--- buildLaunchCommand (workspace-trust pre-write + invocation) ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  // Without skipPermissions: NO trust pre-write — let cursor show its own trust
  // gate. Pre-writing it unconditionally would silently bypass that gate.
  const bare = p.buildLaunchCommand({});
  assert(bare.indexOf(".workspace-trusted") === -1, "bare launch does NOT pre-write the trust marker (gate honored)");
  eq(bare, "cursor-agent", "bare launch is just the invocation, no trust prefix");
  // With skipPermissions (--force, the user's intentional 'trust everything'
  // flow): pre-write the marker so the TUI doesn't block on it.
  const forced = p.buildLaunchCommand({ skipPermissions: true });
  assert(forced.indexOf(".workspace-trusted") !== -1, "skipPermissions pre-writes the trust marker");
  assert(forced.endsWith("cursor-agent --force"), "skipPermissions adds --force after the trust prefix");
  const withModel = p.buildLaunchCommand({ skipPermissions: true, args: ["--model", "auto"] });
  assert(withModel.endsWith("cursor-agent --force --model auto"), "force + model args applied");
  assert(withModel.indexOf("pwd -P") !== -1, "POSIX trust prefix resolves symlinks via pwd -P");
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
  assert(r === "cursor-agent --resume 1a7e8655-f97a-0000-0000-000000000000", "resume by session uuid, no trust prefix");
  const forced = p.buildResumeCommand("abc", true);
  assert(forced.indexOf(".workspace-trusted") !== -1, "resume with skipPermissions pre-writes the trust marker");
  assert(forced.endsWith("cursor-agent --resume abc --force"), "resume carries --force when skipPermissions");
}

console.log("\n--- buildLaunchCommand (windows trust prefix, gated) ---\n");
{
  const p = loadCursorWithHost(baseHost({ platform: () => "windows" }));
  const bare = p.buildLaunchCommand({});
  assert(bare.indexOf(".workspace-trusted") === -1, "windows bare launch does NOT pre-write the trust marker");
  const cmd = p.buildLaunchCommand({ skipPermissions: true });
  assert(cmd.indexOf(".workspace-trusted") !== -1, "windows launch with skipPermissions pre-writes the trust marker");
  assert(cmd.indexOf("Get-Location") !== -1, "windows trust prefix uses PowerShell Get-Location");
  assert(cmd.endsWith("cursor-agent --force"), "windows launch ends with the invocation");
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
        // s2 has neither meta.json nor store.db → empty dir
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
  // s2 has no meta.json or store.db → not a real candidate → only s1 remains → bind it
  eq(p.detectSessionId("/proj"), s1, "empty UUID dir + real session → bind the real one (not refused)");
  eq(p.detectSessionId("/proj", s1), null, "real session excluded → empty dir not a candidate → null");
}

console.log("\n--- discoverModels (live --list-models, manifest fallback) ---\n");
{
  // No CLI available → host.exec returns an error envelope → manifest seed.
  const seedHost = loadCursorWithHost(baseHost({ exec: () => JSON.stringify({ code: 127, error: "not found" }) }));
  eq(seedHost.discoverModels(), manifestSeed.models, "discoverModels falls back to the manifest seed when CLI absent");

  // Real `cursor-agent --list-models` output → parsed id/displayName pairs,
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
  // 2) assistant turn with a redacted-thinking text block + a tool_use block:
  //    the bare [REDACTED] text block is dropped, tool_use survives.
  eq(delta.messages[1].role, "assistant", "row 1 is assistant");
  eq(delta.messages[1].blocks.length, 1, "redacted-only text block dropped, tool_use kept");
  eq(delta.messages[1].blocks[0].kind, "tool_use", "tool_use block preserved");
  eq(delta.messages[1].blocks[0].data.name, "Shell", "tool name preserved");
  eq(delta.messages[1].blocks[0].data.input.command, "echo hello123", "tool input preserved");
  // 3) assistant final text: trailing [REDACTED] trimmed, body kept.
  eq(delta.messages[2].blocks[0].kind, "text", "final assistant block is text");
  eq(delta.messages[2].blocks[0].data.text, "The command printed:\n\n```\nhello123\n```", "trailing [REDACTED] stripped");

  // parseSessionMessages mirrors it for content search (text only, no tool_use).
  const msgs = p.parseSessionMessages(readFixture("transcript.jsonl")) as any[];
  eq(msgs.length, 2, "content search yields the 2 text-bearing turns");
  eq(msgs[0].text, "Run the shell command 'echo hello123' and tell me what it printed.", "search text matches user turn");
  assert(msgs[1].text.indexOf("hello123") !== -1, "search text matches assistant turn");
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
  const base = `/home/u/.cursor/projects/private-tmp-ct-cw2-probe/agent-transcripts/${sid}`;
  const p = loadCursorWithHost(baseHost({
    homeDir: () => "/home/u",
    fs: {
      ...baseHost({}).fs,
      fileExists: () => true,
      walkDir: () => [
        { name: `${sid}.jsonl`, path: `${base}/${sid}.jsonl`, isFile: true, isDir: false, size: 523, createdMs: 1781885008048, modifiedMs: 1781885033657 },
        { name: "repo.json", path: "/home/u/.cursor/projects/private-tmp-ct-cw2-probe/repo.json", isFile: true, isDir: false, size: 50, createdMs: null, modifiedMs: null },
      ],
    },
  }));
  const sessions = p.enumerateSessions() as any[];
  eq(sessions.length, 1, "only the agent-transcript jsonl enumerated (repo.json ignored)");
  eq(sessions[0].sessionId, sid, "session id is the transcript uuid");
  eq(sessions[0].provider, "cursor", "provider tagged cursor");
  eq(sessions[0].createdTs, Math.floor(1781885008048 / 1000), "createdTs from file stat (unix seconds)");
}

console.log("\n--- chat path: JSONL tailer, not structured ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  eq(p.usesStructuredChat(), false, "usesStructuredChat false → host uses the JSONL byte-offset tailer");
  eq(p.parseUsage("any tui screen text", 0), null, "parseUsage null (cursor TUI has no usage gauge)");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
