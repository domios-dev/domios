/**
 * Opencode plugin contract + detection tests.
 * Run: npx tsx plugins/provider-opencode/tests/plugin.test.ts
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
  log: () => {},
  shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
  ...over,
});

console.log("\n--- discoverModels ---\n");

{
  const p = loadOpencodeWithHost(ocHost({
    exec: () => JSON.stringify({ code: 0, stdout: "opencode/live-a\nanthropic/claude-live-b\n" }),
  }));
  eq(
    p.discoverModels(),
    [
      { id: "opencode/live-a", displayName: "OpenCode Live A" },
      { id: "anthropic/claude-live-b", displayName: "Claude Live B" },
    ],
    "discoverModels surfaces the stubbed opencode models probe",
  );
}

{
  const p = loadOpencodeWithHost(ocHost({}));
  eq(
    p.discoverModels(),
    [{ id: "seed-opencode", displayName: "Seed OpenCode", description: "Manifest fallback" }],
    "discoverModels falls back to manifest seed when opencode models probe is unavailable",
  );
}

// Install must write the x-codeterm-binding-nonce header into the generated
// opencode plugin so the session-start report can prove pane ownership.
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
  const res = p.installIntegration(7686);
  assert(!!res && res.ok === true, "installIntegration succeeds", JSON.stringify(res));
  assert(
    body.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 &&
      body.indexOf("x-codeterm-binding-nonce") !== -1,
    "generated opencode plugin sends the binding-nonce header",
  );
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
  const cur = loadOpencodeWithHost(
    withBody("// codeterm-opencode-session-binding-v2\nprocess.env.CODETERM_SESSION_BINDING_NONCE;"),
  );
  assert(
    cur.checkIntegration().plugin_file_current === true,
    "opencode plugin carrying the nonce reads as current",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
