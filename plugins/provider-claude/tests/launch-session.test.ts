/**
 * Hook-independent Claude binding: a process registry record binds only the
 * tab's own process, and only when its process start matches the live process.
 *
 * Run: node node_modules/tsx/dist/cli.mjs plugins/provider-claude/tests/launch-session.test.ts
 */

import { detectClaudeLaunchSession, registrySessionId } from "../src/launchSession";

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}: expected ${e}, got ${a}`); }
}

const files: Record<string, string> = {};
(globalThis as unknown as { host: unknown }).host = {
  platform: () => "linux",
  homeDir: () => "/home/user",
  envGet: () => null,
  fs: { readFile: (path: string) => (path in files ? files[path] : null) },
};

const record = (pid: number, sessionId: string, procStart: unknown, kind = "interactive") =>
  JSON.stringify({ pid, sessionId, procStart, kind });

files["/home/user/.claude/sessions/200.json"] = record(200, "sid-own", "5000");
files["/home/user/.claude/sessions/300.json"] = record(300, "sid-nested", "6000");
files["/home/user/.claude/sessions/400.json"] = record(400, "sid-reused", "1000");

const evidence = (processes: Array<{ pid: number; depth: number; startToken: string }>) => ({
  cwd: "/work",
  launchMarker: null,
  launchedAtMs: 0,
  processes,
});

console.log("\n--- process registry detection ---\n");

eq(
  detectClaudeLaunchSession(evidence([
    { pid: 100, depth: 0, startToken: "4000" },
    { pid: 300, depth: 2, startToken: "6000" },
    { pid: 200, depth: 1, startToken: "5000" },
  ])),
  { sessionId: "sid-own", source: "pid_registry" },
  "the shallowest own process with a record names the session",
);
eq(
  detectClaudeLaunchSession(evidence([{ pid: 400, depth: 1, startToken: "9000" }])),
  null,
  "a reused pid whose start differs names nothing",
);
eq(
  detectClaudeLaunchSession(evidence([{ pid: 500, depth: 1, startToken: "5000" }])),
  null,
  "a process without a record names nothing",
);
eq(detectClaudeLaunchSession(evidence([])), null, "an empty process tree names nothing");

console.log("\n--- record validation ---\n");

eq(registrySessionId(record(200, "sid", "5000", "print"), 200, "5000"), null, "a non-interactive record names nothing");
eq(registrySessionId(record(201, "sid", "5000"), 200, "5000"), null, "a record for another pid names nothing");
eq(registrySessionId(record(200, "sid", 5000), 200, "5000"), null, "a numeric process start is not a start token");
eq(registrySessionId(record(200, "sid", "5000"), 200, ""), null, "an unknown live start names nothing");
eq(registrySessionId("{not json", 200, "5000"), null, "an unreadable record names nothing");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
