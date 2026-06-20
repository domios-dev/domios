/**
 * Codex plugin contract + detection tests.
 * Run: npx tsx plugins/provider-codex/tests/plugin.test.ts
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

function loadCodexWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const manifestSeed = {
  models: [
    { id: "seed-model", displayName: "Seed Model", description: "Manifest fallback" },
  ],
};

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
eq(plugin.meta.id, "codex", "meta.id is 'codex'");
eq(plugin.meta.displayName, "Codex", "meta.displayName");

console.log("\n--- detect ---\n");

const codexFixture = [
  "OpenAI Codex",
  "codex/gpt-5",
  "▶ Provide your answer",
  "ctrl+c to cancel",
].join("\n");

assert(plugin.detect({ bytes: "", title: "codex" }), "title 'codex' positive");
assert(plugin.detect({ bytes: "", title: "Codex CLI" }), "title 'Codex CLI' (case-insensitive)");
assert(plugin.detect({ bytes: codexFixture }), "current real multi-signal output detects");
for (const literal of ["OpenAI Codex", "codex/", "ctrl+c to cancel"]) {
  assert(
    plugin.detect({ bytes: codexFixture.replace(literal, "[changed copy]") }),
    `single literal mutation still detects: ${literal}`
  );
}
assert(!plugin.detect({ bytes: "$ ls\nfile.txt", title: "bash" }), "plain shell negative");
assert(!plugin.detect({ bytes: "claude code", title: "claude" }), "claude title is not codex");

console.log("\n--- statusOf ---\n");
eq(plugin.statusOf({ kind: "anything" }), null, "statusOf always returns null (PTY-driven status)");

console.log("\n--- actions ---\n");
eq(plugin.actions, [], "no actions defined yet");

console.log("\n--- titlePattern + noisePatterns ---\n");
assert(plugin.titlePattern instanceof RegExp, "titlePattern is RegExp");
assert(plugin.titlePattern!.test("codex"), "titlePattern matches 'codex'");
assert(Array.isArray(plugin.noisePatterns) && plugin.noisePatterns.length > 0, "noisePatterns non-empty");

// No image-send strategy → path fallback used by host
assert(plugin.imageSend === undefined, "imageSend undefined (host falls back to path strategy)");

console.log("\n--- hasEventMarkers: picker/menu screens gate a screen read (/model) ---\n");
{
  const evp = loadHostPlugin();
  const modelScreen = fs.readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "model-picker.txt"),
    "utf8",
  );
  assert(evp.hasEventMarkers(modelScreen), "hasEventMarkers fires on the /model picker screen");
  assert(evp.hasEventMarkers("Select Model and Effort"), "fires on the /model title before rows settle");
  assert(evp.hasEventMarkers("Press enter to confirm or esc to go back"), "fires on the confirm/back footer");
  assert(evp.hasEventMarkers("› 1. gpt-5.5 (current)"), "fires on a › selector option row");
  assert(evp.hasEventMarkers("Would you like to run the following command"), "still fires on approval phrasing");
  assert(!evp.hasEventMarkers("Wrote 12 lines to src/lib/foo.ts and ran the tests."), "stays quiet on normal output");
}

console.log("\n--- parsePrompt + buildPromptResponse: /model numbered picker ---\n");
{
  const evp = loadHostPlugin();
  const modelScreen = fs.readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "model-picker.txt"),
    "utf8",
  );
  const parsed = evp.parsePrompt(modelScreen);
  eq(
    parsed && parsed.question,
    "Select Reasoning Level for gpt-5.5",
    "parsePrompt uses the active picker cluster",
  );
  eq(
    parsed && parsed.options,
    [
      ["Low                        Fast responses with lighter reasoning", "1"],
      ["Medium (default) (current) Balances speed and reasoning depth for everyday tasks", "2"],
      ["High                       Greater reasoning depth for complex problems", "3"],
      ["Extra high                 Extra high reasoning depth for complex problems", "4"],
    ],
    "parsePrompt extracts active numbered options with 1-based values",
  );
  eq(evp.buildPromptResponse(1, modelScreen), ["DQ=="], "current highlighted option → Enter only");
  eq(evp.buildPromptResponse(0, modelScreen), ["G1tB", "DQ=="], "target above current → Up + Enter");
  eq(evp.buildPromptResponse(3, modelScreen), ["G1tC", "G1tC", "DQ=="], "target below current → Down ×2 + Enter");
}

console.log("\n--- parsePrompt + buildPromptResponse: prose choice prompts ---\n");
{
  const evp = loadHostPlugin();
  const abc = [
    "• Which release strategy should we use?",
    "  A) Fast patch release now",
    "  B) Minor release with bundled fixes",
    "  C) Delay for more testing",
  ].join("\n");
  const parsed = evp.parsePrompt(abc);
  eq(parsed && parsed.question, "Which release strategy should we use?", "lettered prose question extracts");
  eq(
    parsed && parsed.options,
    [
      ["Fast patch release now", "A"],
      ["Minor release with bundled fixes", "B"],
      ["Delay for more testing", "C"],
    ],
    "lettered prose options extract",
  );
  eq(evp.buildPromptResponse(1, abc), ["Qg0="], "lettered click sends selected value plus Enter");

  const numbered = [
    "• Which test strategy should we use?",
    "",
    "  1. Unit tests first",
    "  2. Integration smoke tests",
    "  3. Full end-to-end suite",
  ].join("\n");
  const parsedNumbered = evp.parsePrompt(numbered);
  eq(parsedNumbered && parsedNumbered.question, "Which test strategy should we use?", "numbered prose question extracts");
  eq(evp.buildPromptResponse(2, numbered), ["Mw0="], "numbered click sends selected value plus Enter");
}

console.log("\n--- parseSessionDelta hardening ---\n");
const hostPlugin = loadHostPlugin();
const chunk = [
  JSON.stringify({
    type: "response_item",
    timestamp: "2026-01-01T00:00:00Z",
    payload: { type: "message", role: "assistant", content: [{ text: "ok", type: "text" }] },
  }),
  JSON.stringify({ type: "response_item", timestamp: "2026-01-01T00:00:00Z", payload: {} }),
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

{
  const sparseChunk = [
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-01-01T00:00:00Z",
      payload: {
        type: "function_call",
        name: "exec_command",
        arguments: JSON.stringify({ cmd: "npm test" }),
      },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-01-01T00:00:01Z",
      payload: { type: "function_call", arguments: "{}" },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-01-01T00:00:02Z",
      payload: {
        type: "function_call",
        name: "exec_command",
        arguments: JSON.stringify({ cmd: "npm run build" }),
      },
    }),
  ].join("\n");
  const sparseParsed = hostPlugin.parseSessionDelta(sparseChunk) as { messages: any[] };
  eq(sparseParsed.messages.length, 2, "parseSessionDelta filters empty sparse function_call rows");
  eq(
    sparseParsed.messages.map((message) => message.blocks[0].data.input.command),
    ["npm test", "npm run build"],
    "parseSessionDelta keeps real exec_command rows",
  );
}

console.log("\n--- discoverModels ---\n");

{
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (p: string) => p.endsWith("/.codex/models_cache.json"),
      readFile: () => null,
      readJson: (p: string) => p.endsWith("/.codex/models_cache.json")
        ? { models: [{ slug: "gpt-live", display_name: "GPT Live", description: "from cache" }] }
        : null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  eq(
    p.discoverModels(),
    [{ id: "gpt-live", displayName: "GPT Live", description: "from cache" }],
    "discoverModels surfaces the stubbed Codex cache probe",
  );
}

{
  const p = loadCodexWithHost(baseHost({}));
  eq(p.discoverModels(), manifestSeed.models, "discoverModels falls back to manifest seed when no Codex cache exists");
}

// Live cache → clean, current, priority-ordered chat models: hidden + non-chat dropped.
{
  const cache = {
    models: [
      { slug: "gpt-5.4", display_name: "GPT-5.4", description: "everyday", visibility: "list", priority: 16 },
      { slug: "codex-auto-review", display_name: "Auto Review", visibility: "hide", priority: 43 },
      { slug: "text-embedding-3-large", display_name: "Embeds", visibility: "list", priority: 5 },
      { slug: "gpt-5.5", display_name: "GPT-5.5", description: "frontier", visibility: "list", priority: 9 },
    ],
  };
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (p: string) => p.endsWith("/.codex/models_cache.json"),
      readFile: () => null,
      readJson: (p: string) => p.endsWith("/.codex/models_cache.json") ? cache : null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  const models = p.discoverModels() as Array<{ id: string; available?: boolean }>;
  eq(models.map((m) => m.id), ["gpt-5.5", "gpt-5.4"], "visibility:hide + embedding dropped; sorted by ascending priority");
  assert(models.every((m) => m.available === undefined), "codex has no plan-lockout signal → available unset");
}

console.log("\n--- binding nonce (install + outdated detection) ---\n");

// Install must emit the X-CodeTerm-Binding-Nonce header so the provider's
// session-start report can prove pane ownership (release/1.4.6 binding-nonce).
{
  let writtenJson: any = null;
  const host = baseHost({
    fs: {
      fileExists: () => false,
      readJson: () => null,
      readFile: () => null,
      writeJson: (_p: string, json: unknown) => { writtenJson = json; return true; },
      writeFile: () => true,
    },
  });
  const p = loadCodexWithHost(host);
  const res = p.installIntegration(7686);
  assert(!!res && res.ok === true, "installIntegration succeeds", JSON.stringify(res));
  const cmd = String(writtenJson?.hooks?.SessionStart?.[0]?.hooks?.[0]?.command ?? "");
  assert(
    cmd.indexOf("X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}") !== -1,
    "session_start hook curl carries the binding-nonce header",
    cmd,
  );
}

// A hook lacking the nonce header is pre-nonce → must read as not-current so
// startup reinstalls it; a hook carrying it is current.
{
  const sessionStartHook = (cmd: string) =>
    baseHost({
      fs: {
        fileExists: () => true,
        readFile: () => "",
        readJson: () => ({ hooks: { SessionStart: [{ hooks: [{ command: cmd }] }] } }),
        writeJson: () => true,
        writeFile: () => true,
      },
    });
  const preNonce =
    "curl -s -X POST http://localhost:7686/api/panes/$CODETERM_PANE_ID/hook/session_start " +
    "-H 'X-CodeTerm-Agent-Provider: codex' --data-binary @-";
  const old = loadCodexWithHost(sessionStartHook(preNonce));
  assert(
    old.checkIntegration().session_start_hook === false,
    "pre-nonce hook is reported not-current (forces reinstall)",
  );
  const current = loadCodexWithHost(
    sessionStartHook(preNonce + ' -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"'),
  );
  assert(
    current.checkIntegration().session_start_hook === true,
    "hook carrying the binding-nonce header reads as current",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
