import { ioTestExport } from "../../provider-shared/tests/hostIo";
/**
 * Hook-independent Claude binding: a process registry record binds only the
 * tab's own process, and only when its process start matches the live process.
 *
 * Run: node node_modules/tsx/dist/cli.mjs plugins/provider-claude/tests/launch-session.test.ts
 */

import { detectClaudeLaunchSession as ioSource_detectClaudeLaunchSession, registrySessionId } from "../src/launchSession";
const detectClaudeLaunchSession = ioTestExport(ioSource_detectClaudeLaunchSession);

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}: expected ${e}, got ${a}`); }
}

const files: Record<string, string> = {};
let platform = "linux";
let registryModifiedMs: number | null = null;
let replacement: string | null = null;
const diagnostics: string[] = [];
const hostFixture = {
  platform: () => platform,
  homeDir: () => "/home/user",
  envGet: () => null,
  log: (_level: string, message: string) => diagnostics.push(message),
  fs: {
    readFile: (path: string) => files[path] ?? null,
    readDir: (dir: string) => {
      const path = dir + "/2860.json";
      if (replacement !== null) files[path] = replacement;
      return [{ name: "2860.json", isFile: true, modifiedMs: registryModifiedMs }];
    },
  },
};
(globalThis as unknown as { host: unknown }).host = hostFixture;

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

const macStart = Date.UTC(2026, 10, 1, 5, 30, 0) * 1_000 + 789;
const macRecord = record(2860, "sid-macos", "Sun Nov  1 05:30:00 2026");
const startMs = Math.floor(macStart / 1_000);
const freshMs = startMs + 3_600_001;
eq(registrySessionId(macRecord, 2860, String(macStart), freshMs), "sid-macos", "macOS matches a fresh record with a pinned UTC timestamp");
eq(registrySessionId(macRecord, 2860, String(macStart + 1_000_000), freshMs), null, "macOS rejects a reused pid with a different start");
eq(registrySessionId(macRecord, 2860, String(macStart)), null, "the macOS format is platform scoped");
const later = macStart + 3_600_000_000;
eq(registrySessionId(macRecord, 2860, String(later), freshMs), null, "UTC keeps both sides of the DST fold distinct");
eq(registrySessionId(record(2860, "later", "Sun Nov  1 06:30:00 2026"), 2860, String(later), freshMs), "later",
  "a process started after the DST transition has its own UTC second");
for (const token of [macStart, later]) {
  eq(registrySessionId(record(2860, "stale", "Sun Nov  1 01:30:00 2026"), 2860, String(token), freshMs), null,
    "a legacy local timestamp in the DST fold cannot identify either process");
  eq(registrySessionId(record(2860, "unpin", "dim.  1 nov. 01:30:00 2026"), 2860, String(token), freshMs), null,
    "a localized legacy timestamp cannot identify either process");
}
platform = "macos";
const rigaCollision = record(2860, "sid-macos", "Sat Aug  1 05:30:00 2026");
const reusedStartMs = Date.UTC(2026, 7, 1, 5, 30, 0);
const reusedStart = reusedStartMs * 1_000 + 789;
files["/home/user/.claude/sessions/2860.json"] = rigaCollision;
const macEvidence = evidence([{ pid: 2860, depth: 1, startToken: String(reusedStart) }]);
eq(detectClaudeLaunchSession(macEvidence), null, "formatted records require file freshness evidence");
registryModifiedMs = Date.UTC(2026, 7, 1, 2, 30, 0, 10);
eq(detectClaudeLaunchSession(macEvidence), null, "a stale Riga timestamp cannot identify a reused PID");
registryModifiedMs = reusedStartMs;
eq(detectClaudeLaunchSession(macEvidence), null, "millisecond rounding must leave the write strictly after the start");
registryModifiedMs = reusedStartMs + 1;
eq(detectClaudeLaunchSession(macEvidence), { sessionId: "sid-macos", source: "pid_registry" },
  "the same UTC timestamp identifies a record written after the exact process start");
replacement = record(2860, "new-session", "Sat Aug  1 05:30:00 2026");
eq(detectClaudeLaunchSession(macEvidence), null, "a replacement cannot lend its freshness to the earlier content");
for (const reason of ["missing_record_mtime", "stale_record", "record_changed"]) {
  eq(diagnostics.some(message => message.endsWith(reason)), true, `registry logs ${reason}`);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
