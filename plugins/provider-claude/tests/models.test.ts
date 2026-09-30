/**
 * Claude model discovery tests: the CLI-version gate and the manifest's static
 * `available` seed are two independent mechanisms, and neither may mask the
 * other. Fable shipped hidden for months because `plugin.json` carried a
 * hardcoded `"available": false` — a static flag has no business overriding a
 * dynamic, testable gate, and a regression that reintroduces one must not be
 * able to hide a model the gate would otherwise allow.
 * Run: npx tsx plugins/provider-claude/tests/models.test.ts
 *
 * No test here asserts on a real display name/version string beyond what the
 * fixture itself supplies — assertions are on the gate mechanism, never on a
 * pinned model list, so this file survives the next Anthropic release.
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

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

function loadClaudeWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

// The bundled manifest IS the shipped alias seed — tests read it from disk so
// a drifted copy here can never mask a stale plugin.json.
const manifestSeed = JSON.parse(fs.readFileSync(
  path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.json"),
  "utf8",
)) as { models: Array<{ id: string; displayName: string; description?: string; available?: boolean }> };

const baseHost = (over: Record<string, unknown>) => ({
  manifestJson: () => JSON.stringify(manifestSeed),
  homeDir: () => "/tmp",
  path: { equal: () => true },
  platform: () => "linux",
  unixNowMs: () => 0,
  envGet: () => null,
  exec: () => JSON.stringify({ error: "not configured" }),
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

console.log("\n--- discoverModels: shipped manifest has no static veto over the gate ---\n");

{
  // A hardcoded manifest `available` is a static veto over a dynamic gate,
  // which is the regression this file guards: MINIMUM_CLI_BY_MODEL decides.
  assert(
    !manifestSeed.models.some((m) => m.available === false),
    "shipped plugin.json does not hardcode any model unavailable",
  );
}

console.log("\n--- discoverModels: genuine discovery only ---\n");

{
  const p = loadClaudeWithHost(baseHost({
    exec: () => JSON.stringify({ code: 0, stdout: "2.1.258 (Claude Code)" }),
    envGet: (name: string) => name === "ANTHROPIC_API_KEY" ? "test-key" : null,
    fetch: () => JSON.stringify({
      status: 200,
      body: JSON.stringify({
        data: [
          { id: "claude-opus-5", display_name: "Claude Opus 5" },
          { id: "claude-sonnet-5", display_name: "Claude Sonnet 5" },
          { id: "claude-haiku-4-5", display_name: "Claude Haiku 4.5" },
          { id: "claude-fable-5-1", display_name: "Claude Fable 5.1" },
        ],
      }),
    }),
  }));
  eq(
    p.discoverModels(),
    [
      { id: "opus", displayName: "Claude Opus 5", description: "Best for everyday, complex tasks.", available: true },
      { id: "sonnet", displayName: "Claude Sonnet 5", description: "Efficient for routine tasks.", available: true },
      { id: "haiku", displayName: "Claude Haiku 4.5", description: "Fastest for quick answers.", available: true },
      { id: "fable", displayName: "Claude Fable 5.1", description: "Most capable for your hardest and longest-running tasks.", available: true, availability: "available" },
      { id: "best", displayName: "Claude Best 5.1", description: "Claude Code's dynamic best-model alias.", available: true },
      { id: "sonnet[1m]", displayName: "Claude Sonnet (1M context)", description: "Claude Code's Sonnet long-context alias.", available: true },
      { id: "opus[1m]", displayName: "Claude Opus (1M context)", description: "Claude Code's Opus long-context alias.", available: true },
      { id: "fable[1m]", displayName: "Claude Fable (1M context)", description: "Claude Code's Fable long-context alias.", available: true },
      { id: "opusplan", displayName: "Claude Opus Plan 5", description: "Claude Code's Opus planning alias.", available: true },
    ],
    "genuine /v1/models discovery maps concrete ids onto the same stable aliases with live availability",
  );
}

{
  const p = loadClaudeWithHost(baseHost({
    exec: () => JSON.stringify({ code: 0, stdout: "2.1.258 (Claude Code)" }),
    envGet: (name: string) => name === "ANTHROPIC_API_KEY" ? "test-key" : null,
    fetch: () => JSON.stringify({
      status: 200,
      body: JSON.stringify({ data: [{ id: "claude-fable-5-1", display_name: "Claude Fable 5.1" }] }),
    }),
  }));
  const partial = p.discoverModels() as Array<{ id: string; displayName: string; available: boolean }>;
  eq(
    partial.map((m) => m.id),
    ["opus", "sonnet", "haiku", "fable", "best", "sonnet[1m]", "opus[1m]", "fable[1m]", "opusplan"],
    "partial discovery keeps every CLI alias stable",
  );
  eq(
    partial.map((m) => m.available),
    [false, false, false, true, true, false, false, true, false],
    "partial discovery publishes family availability for aliases",
  );
  eq(
    partial.map((m) => m.displayName),
    [
      "Claude Opus",
      "Claude Sonnet",
      "Claude Haiku",
      "Claude Fable 5.1",
      "Claude Best 5.1",
      "Claude Sonnet (1M context)",
      "Claude Opus (1M context)",
      "Claude Fable (1M context)",
      "Claude Opus Plan",
    ],
    "family aliases receive their concrete version without rewriting context labels",
  );
}

{
  const p = loadClaudeWithHost(baseHost({
    exec: () => JSON.stringify({ code: 0, stdout: "2.1.71 (Claude Code)" }),
    fetch: () => { throw new Error("network must not be touched without credentials"); },
  }));
  const models = p.discoverModels() as Array<{ id: string; available?: boolean }>;
  eq(models.find((model) => model.id === "fable")?.available, false, "installed CLI without fable support does not advertise it");
}

{
  const p = loadClaudeWithHost(baseHost({
    exec: () => JSON.stringify({ code: 0, stdout: "2.1.258 (Claude Code)" }),
    fetch: () => { throw new Error("network must not be touched without credentials"); },
  }));
  const models = p.discoverModels() as Array<{ id: string; available?: boolean }>;
  eq(models.find((model) => model.id === "fable")?.available, true, "installed CLI with fable support advertises it without requiring an API-key probe");
}

// A dead network says nothing about the installed CLI, and this fixture's probe
// cannot run either, so the honest answer is unknown, never the greying-out no.
for (const [label, fetchImpl] of [
  ["fetch failure", () => { throw new Error("offline"); }],
  ["non-200 response", () => JSON.stringify({ status: 503, body: "overloaded" })],
  ["malformed body", () => JSON.stringify({ status: 200, body: "not json" })],
  ["empty usable list", () => JSON.stringify({ status: 200, body: JSON.stringify({ data: [{ id: "  " }] }) })],
] as Array<[string, () => string]>) {
  const p = loadClaudeWithHost(baseHost({
    envGet: (name: string) => name === "CLAUDE_API_KEY" ? "test-key" : null,
    fetch: fetchImpl,
  }));
  const models = p.discoverModels() as Array<{ id: string; available?: boolean; availability?: string }>;
  const fable = models.find((model) => model.id === "fable");
  eq(fable?.availability, "unknown", `an unreachable CLI probe reports unknown on ${label}`);
  eq(fable?.available, undefined, `an unreachable CLI probe publishes no boolean on ${label}`);
  eq("available" in (fable as object), false, `the unknown verdict carries no available key on ${label}`);
}

console.log("\n--- discoverModels: could-not-run is not answered-no ---\n");

for (const [label, execImpl] of [
  ["exec throws", () => { throw new Error("spawn claude: not found"); }],
  ["exec reports a non-zero exit", () => JSON.stringify({ code: 1, stdout: "" })],
  ["exec output carries no version", () => JSON.stringify({ code: 0, stdout: "claude" })],
  ["exec returns junk", () => "not json"],
] as Array<[string, () => string]>) {
  const p = loadClaudeWithHost(baseHost({
    exec: execImpl,
    fetch: () => { throw new Error("network must not be touched without credentials"); },
  }));
  const models = p.discoverModels() as Array<{ id: string; available?: boolean; availability?: string }>;
  const fable = models.find((model) => model.id === "fable");
  eq(fable?.availability, "unknown", `${label} yields unknown, not unavailable`);
  eq(fable?.available, undefined, `${label} publishes no availability boolean`);
}

{
  // The other half of the same rule: a CLI that genuinely answered no is still
  // a definite negative, so unknown does not swallow real unavailability.
  const p = loadClaudeWithHost(baseHost({
    exec: () => JSON.stringify({ code: 0, stdout: "2.1.71 (Claude Code)" }),
    fetch: () => { throw new Error("network must not be touched without credentials"); },
  }));
  const models = p.discoverModels() as Array<{ id: string; available?: boolean; availability?: string }>;
  const fable = models.find((model) => model.id === "fable");
  eq(fable?.availability, "unavailable", "a CLI below the gate answered no");
  eq(fable?.available, false, "a genuine negative still publishes the boolean");
}

console.log("\n--- discoverModels: manifest flag and version gate are independent ---\n");

{
  // plugin.json once hardcoded fable unavailable while the CLI gate was already
  // satisfied, so a manifest veto must not mask a satisfied version gate.
  const staleManifest = {
    models: [
      { id: "fable", displayName: "Claude Fable", description: "desc", available: false },
    ],
  };
  const p = loadClaudeWithHost(baseHost({
    manifestJson: () => JSON.stringify(staleManifest),
    exec: () => JSON.stringify({ code: 0, stdout: "2.1.261 (Claude Code)" }),
    fetch: () => { throw new Error("network must not be touched without credentials"); },
  }));
  const models = p.discoverModels() as Array<{ id: string; available?: boolean }>;
  eq(
    models.find((m) => m.id === "fable")?.available,
    true,
    "a manifest-hardcoded available:false cannot mask a version gate the installed CLI already satisfies",
  );
}

{
  // Conversely, a manifest that never mentions `available` at all (the
  // permissive/default shape) must not mask a failing version gate.
  const permissiveManifest = {
    models: [
      { id: "fable", displayName: "Claude Fable", description: "desc" },
    ],
  };
  const p = loadClaudeWithHost(baseHost({
    manifestJson: () => JSON.stringify(permissiveManifest),
    exec: () => JSON.stringify({ code: 0, stdout: "2.1.0 (Claude Code)" }),
    fetch: () => { throw new Error("network must not be touched without credentials"); },
  }));
  const models = p.discoverModels() as Array<{ id: string; available?: boolean }>;
  eq(
    models.find((m) => m.id === "fable")?.available,
    false,
    "a permissive manifest (no available field) cannot mask a version gate the installed CLI fails",
  );
}

console.log(`
${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
