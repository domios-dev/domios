/**
 * Cursor plugin contract + detection tests.
 * Run: npx tsx plugins/provider-cursor/tests/plugin.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import plugin from "../src/index";

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
  log: () => {},
  shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
  fs: {
    fileExists: () => false,
    readFile: () => null,
    readJson: () => null,
    writeJson: () => true,
    writeFile: () => true,
  },
  ...over,
});

console.log("\n--- meta ---\n");
eq(plugin.meta.id, "cursor", "meta.id is 'cursor'");
eq(plugin.meta.displayName, "Cursor", "meta.displayName");

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

console.log("\n--- buildLaunchCommand ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  eq(p.buildLaunchCommand({}), "cursor-agent", "bare launch is just the binary");
  eq(p.buildLaunchCommand({ skipPermissions: true }), "cursor-agent --force", "skipPermissions adds --force");
  eq(
    p.buildLaunchCommand({ skipPermissions: true, args: ["--model", "auto"] }),
    "cursor-agent --force --model auto",
    "force + model args applied",
  );
}

console.log("\n--- buildResumeCommand ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  eq(
    p.buildResumeCommand("1a7e8655-f97a-0000-0000-000000000000"),
    "cursor-agent --resume 1a7e8655-f97a-0000-0000-000000000000",
    "resume by session uuid",
  );
  eq(
    p.buildResumeCommand("abc", true),
    "cursor-agent --resume abc --force",
    "resume carries --force when skipPermissions",
  );
}

console.log("\n--- discoverModels ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  eq(p.discoverModels(), manifestSeed.models, "discoverModels returns the manifest seed (no list command)");
}

console.log("\n--- CW2 stubs are valid no-ops ---\n");
{
  const p = loadCursorWithHost(baseHost({}));
  eq(p.usesStructuredChat(), false, "usesStructuredChat false until CW2");
  eq(p.enumerateSessions(), [], "enumerateSessions empty until CW2");
  eq(p.sessionExists("/tmp", "x"), false, "sessionExists false until CW2");
  eq(p.detectSessionModel("/tmp", "x"), null, "detectSessionModel null until CW2");
  eq(p.parseUsage("", 0), null, "parseUsage null until CW2");
  eq(p.parseSessionMessages(""), [], "parseSessionMessages empty until CW2");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
