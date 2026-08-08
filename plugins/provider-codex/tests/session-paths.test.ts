/**
 * Codex session-path resolution tests: the rollout's own session_meta id is
 * provider-owned evidence and wins over filename reconstruction
 * (transcript_path drift, DN-2).
 * Run: npx tsx plugins/provider-codex/tests/session-paths.test.ts
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

function loadCodexWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const baseHost = (over: Record<string, unknown>) => ({
  manifestJson: () => "null",
  homeDir: () => "/tmp/home",
  platform: () => "linux",
  unixNowMs: () => 1_000_000,
  path: { equal: (a: string, b: string) => a === b },
  expandHome: (v: string) => v.replace(/^~/, "/tmp/home"),
  envGet: (k: string) => (k === "CODETERM_CODEX_SESSIONS_ROOT" ? "/sessions" : null),
  log: () => {},
  shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
  fs: {
    fileExists: () => true,
    readFile: () => null,
    readFileHead: () => null,
    readFileTail: () => null,
    readJson: () => null,
    writeJson: () => true,
    writeFile: () => true,
    writeTempFile: () => "/tmp/codeterm-task.md",
    walkDir: () => [],
  },
  ...over,
});

console.log("\n--- sessionFilePath: session_meta stored id wins over filename (transcript_path drift) ---\n");
{
  // Provider-owned evidence: the rollout header names the real session id even
  // when the vendor named the file with a different uuid.
  const driftedMeta = '{"type":"session_meta","payload":{"id":"44444444-4444-4444-4444-444444444444","cwd":"/work/proj"}}';
  const drifted = {
    isFile: true,
    name: "rollout-2026-06-01T10-00-00-99999999-9999-9999-9999-999999999999.jsonl",
    path: "/sessions/drifted.jsonl",
    modifiedMs: 1000,
  };
  const bare = {
    isFile: true,
    name: "rollout-2026-06-01T11-00-00-55555555-5555-5555-5555-555555555555.jsonl",
    path: "/sessions/bare.jsonl",
    modifiedMs: 2000,
  };
  const p = loadCodexWithHost(baseHost({
    fs: {
      ...baseHost({}).fs,
      readFileHead: (file: string) => (file === "/sessions/drifted.jsonl" ? driftedMeta + "\n" : null),
      walkDir: () => [drifted, bare],
    },
  }));
  eq(
    p.sessionFilePath("/work/proj", "44444444-4444-4444-4444-444444444444"),
    "/sessions/drifted.jsonl",
    "stored session_meta id resolves the rollout when the filename drifted",
  );
  eq(
    p.sessionFilePath("/work/proj", "99999999-9999-9999-9999-999999999999"),
    null,
    "a session_meta-bearing rollout is never matched by its drifted filename",
  );
  eq(
    p.sessionFilePath("/work/proj", "55555555-5555-5555-5555-555555555555"),
    "/sessions/bare.jsonl",
    "rollouts without session_meta keep today's stem reconstruction",
  );
  eq(
    p.sessionJsonlPath("/work/proj", "44444444-4444-4444-4444-444444444444"),
    "/sessions/drifted.jsonl",
    "chat tail path resolves through the same stored-id preference",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
