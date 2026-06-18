/**
 * Plugin contract tests for Claude — meta, statusOf, actions, detect.
 * Run: npx tsx plugins/provider-claude/tests/plugin.test.ts
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

function loadHostPlugin() {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = {
    host: {
      manifestJson: () => "null",
      homeDir: () => "/tmp",
      path: { equal: () => true },
      platform: () => "linux",
      unixNowMs: () => 0,
      envGet: () => null,
      shell: {
        quoteFor: (value: unknown) => String(value),
        quote: (value: unknown) => String(value),
      },
      fs: {
        fileExists: () => false,
        readFile: () => null,
        readJson: () => null,
        writeTempFile: () => "/tmp/ct-task",
        writeJson: () => true,
      },
      expandHome: (value: string) => value,
      writeFile: () => true,
      removeFile: () => true,
      log: () => {},
    },
    console,
    module: { exports: {} },
    exports: {},
  };
  const script = new vm.Script(source, { filename: pluginPath });
  script.runInNewContext(context);
  return context.module.exports.default;
}

function loadClaudeWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const manifestSeed = {
  models: [
    { id: "seed-sonnet", displayName: "Seed Sonnet", description: "Manifest fallback" },
  ],
};

const baseHost = (over: Record<string, unknown>) => ({
  manifestJson: () => JSON.stringify(manifestSeed),
  homeDir: () => "/tmp",
  path: { equal: () => true },
  platform: () => "linux",
  unixNowMs: () => 0,
  envGet: () => null,
  fetch: () => JSON.stringify({ error: "not configured" }),
  shell: {
    quoteFor: (value: unknown) => String(value),
    quote: (value: unknown) => String(value),
  },
  fs: {
    fileExists: () => false,
    readFile: () => null,
    readJson: () => null,
    writeTempFile: () => "/tmp/ct-task",
    writeJson: () => true,
  },
  expandHome: (value: string) => value,
  writeFile: () => true,
  removeFile: () => true,
  log: () => {},
  ...over,
});

console.log("\n--- meta ---\n");

eq(plugin.meta.id, "claude", "meta.id is 'claude'");
eq(plugin.meta.displayName, "Claude Code", "meta.displayName");

console.log("\n--- statusOf delegation ---\n");

eq(
  plugin.statusOf({ kind: "user_message" }),
  { kind: "agent_working" },
  "plugin.statusOf delegates to status.ts"
);

console.log("\n--- actions ---\n");

assert(plugin.actions.length >= 2, "has at least 2 actions");

const compact = plugin.actions.find(a => a.id === "compact");
assert(compact !== undefined, "has 'compact' action");
eq(compact?.label, "/compact", "compact action label");
eq(compact?.run({ events: [] }), "/compact\n", "compact action returns slash-command with newline");

const exit = plugin.actions.find(a => a.id === "exit");
assert(exit !== undefined, "has 'exit' action");
eq(exit?.run({ events: [] }), "/exit\n", "exit action returns slash-command with newline");

console.log("\n--- detect delegation ---\n");

assert(plugin.detect({ bytes: "", title: "claude" }), "plugin.detect delegates to detect.ts");

console.log("\n--- imageSend strategy ---\n");

eq(plugin.imageSend?.method, "clipboard-paste", "imageSend method is clipboard-paste");
eq(plugin.imageSend?.pasteKeystroke, "\x16", "imageSend pasteKeystroke is Ctrl+V (0x16)");

console.log("\n--- titlePattern + noisePatterns exposed ---\n");

assert(plugin.titlePattern instanceof RegExp, "titlePattern is a RegExp");
assert(Array.isArray(plugin.noisePatterns) && plugin.noisePatterns.length > 0, "noisePatterns is non-empty array");

console.log("\n--- parseSessionDelta hardening ---\n");
const hostPlugin = loadHostPlugin();
const chunk = [
  JSON.stringify({ type: "assistant", uuid: "ok", message: { content: [{ type: "text", text: "good" }] } }),
  '{"type":"assistant","message":"not-an-array-or-string"}',
  "",
].join("\n");
let warned = 0;
let parsed: { messages: unknown[] } = { messages: [] };
const origWarn = console.warn;
try {
  console.warn = () => warned++;
  parsed = hostPlugin.parseSessionDelta(chunk);
} catch (e) {
  assert(false, "parseSessionDelta does not throw on malformed entry", String(e));
} finally {
  console.warn = origWarn;
}
assert(parsed.messages.length === 1, "parseSessionDelta corrupted lines are skipped");
assert(warned === 1, "parseSessionDelta emits one warning for a session with invalid entries");

console.log("\n--- discoverModels ---\n");

{
  const p = loadClaudeWithHost(baseHost({
    envGet: (name: string) => name === "ANTHROPIC_API_KEY" ? "test-key" : null,
    fetch: () => JSON.stringify({
      status: 200,
      body: JSON.stringify({
        data: [{ id: "claude-live-1", display_name: "Claude Live 1" }],
      }),
    }),
  }));
  eq(
    p.discoverModels(),
    [{ id: "claude-live-1", displayName: "Claude Live 1" }],
    "discoverModels surfaces the stubbed Anthropic models probe",
  );
}

{
  const p = loadClaudeWithHost(baseHost({}));
  eq(p.discoverModels(), manifestSeed.models, "discoverModels falls back to manifest seed when no Anthropic key exists");
}

console.log("\n--- parseUsage reset formats ---\n");

const now = new Date(2026, 5, 14, 15, 0, 0, 0).getTime();
const usage = hostPlugin.parseUsage(
  [
    "Current session 100% usedResets 3:30pm (Europe/Kiev)",
    "Current week (all models) 64% usedResets Jun 14, 2026 at 4 PM (Europe/Kiev)",
  ].join("\n"),
  now,
);
assert(usage?.weekly_pct === 64, "parseUsage extracts weekly pct");
assert(usage?.weekly_resets_at_ms === new Date(2026, 5, 14, 16, 0, 0, 0).getTime(), "parseUsage accepts comma/year reset date");

const sameDayUsage = hostPlugin.parseUsage(
  [
    "Current session 100% usedResets 3:30pm (Europe/Kiev)",
    "Current week (all models) 64% usedResets 4 PM (Europe/Kiev)",
  ].join("\n"),
  now,
);
assert(sameDayUsage?.weekly_resets_at_ms === new Date(2026, 5, 14, 16, 0, 0, 0).getTime(), "parseUsage accepts bare weekly reset time");

const claudeCurrentUsage = hostPlugin.parseUsage(
  [
    "Current session 100% usedResets 3:39pm (Europe/Kiev)",
    "Current week (all models) 64% usedResets Jun 14, 7:59pm (Europe/Kiev)",
  ].join("\n"),
  now,
);
assert(claudeCurrentUsage?.weekly_resets_at_ms === new Date(2026, 5, 14, 19, 59, 0, 0).getTime(), "parseUsage accepts month-day comma time");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
