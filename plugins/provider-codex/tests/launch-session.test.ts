/**
 * Hook-independent Codex binding: a thread binds a tab only when its rollout's
 * early launch text carries that launch's marker.
 *
 * Run: node node_modules/tsx/dist/cli.mjs plugins/provider-codex/tests/launch-session.test.ts
 */

import { detectCodexLaunchSession, withLaunchMarker } from "../src/launchSession";
import plugin from "../src/plugin";

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}: expected ${e}, got ${a}`); }
}

const MARKER = "ct-launch-0123456789abcdef0123456789abcdef";
const OTHER_MARKER = "ct-launch-fedcba9876543210fedcba9876543210";
const DB = "/codex/state_5.sqlite";

interface Thread { id: string; rollout_path: string; created_at: number }

const threads: Thread[] = [];
const files: Record<string, string> = {};
const written: string[] = [];

function rollout(id: string, developerText: string, userText = "starter"): string {
  return [
    JSON.stringify({ type: "session_meta", payload: { id, cwd: "/work" } }),
    JSON.stringify({ type: "event_msg", payload: { type: "task_started" } }),
    JSON.stringify({ type: "response_item", payload: { type: "message", role: "developer", content: [{ type: "input_text", text: developerText }] } }),
    JSON.stringify({ type: "response_item", payload: { type: "message", role: "user", content: [{ type: "input_text", text: userText }] } }),
  ].join("\n") + "\n";
}

function addThread(id: string, createdAt: number, text: string) {
  const path = `/codex/sessions/rollout-${id}.jsonl`;
  threads.push({ id, rollout_path: path, created_at: createdAt });
  files[path] = rollout(id, text);
}

(globalThis as unknown as { host: unknown }).host = {
  platform: () => "linux",
  homeDir: () => "/home/user",
  envGet: (name: string) => (name === "CODETERM_CODEX_STATE_DB_PATH" ? DB : null),
  shell: { quoteFor: (value: unknown) => `'${String(value)}'` },
  fs: {
    fileExists: (path: string) => path === DB || path in files,
    readFileHead: (path: string) => (path in files ? files[path] : null),
    writeTempFile: (contents: string) => { written.push(contents); return "/tmp/ct-task.md"; },
    sqliteQuery: (_db: string, _sql: string, params: unknown[]) =>
      threads
        .filter((thread) => thread.created_at >= Number(params[0]))
        .sort((a, b) => b.created_at - a.created_at),
  },
};

const LAUNCHED_AT_MS = 1_789_000_000_000;
const evidence = (launchMarker: string | null) => ({
  cwd: "/work",
  launchMarker,
  launchedAtMs: LAUNCHED_AT_MS,
  processes: [],
});

addThread("thread-own", 1_789_000_010, withLaunchMarker("assigned task", MARKER));
addThread("thread-other", 1_789_000_020, withLaunchMarker("assigned task", OTHER_MARKER));
addThread("thread-before-launch", 1_788_999_000, withLaunchMarker("assigned task", MARKER));
addThread("thread-unmarked", 1_789_000_030, "assigned task");

console.log("\n--- launch marker detection ---\n");

eq(
  detectCodexLaunchSession(evidence(MARKER)),
  { sessionId: "thread-own", source: "launch_marker" },
  "the thread whose launch text carries this launch's marker binds",
);
eq(detectCodexLaunchSession(evidence(null)), null, "a launch without a marker names nothing");
eq(detectCodexLaunchSession(evidence("short")), null, "a trivially short marker names nothing");
eq(
  detectCodexLaunchSession(evidence("ct-launch-00000000000000000000000000000000")),
  null,
  "a marker no rollout carries names nothing",
);

files["/codex/sessions/rollout-thread-other.jsonl"] = rollout("thread-renamed", withLaunchMarker("task", OTHER_MARKER));
eq(
  detectCodexLaunchSession(evidence(OTHER_MARKER)),
  null,
  "a rollout whose session_meta names another thread names nothing",
);

addThread("thread-duplicate", 1_789_000_040, withLaunchMarker("assigned task", MARKER));
eq(detectCodexLaunchSession(evidence(MARKER)), null, "two threads carrying one marker are ambiguous, not a guess");

console.log("\n--- launch command carries the marker ---\n");

written.length = 0;
plugin.buildLaunchCommand!({ task: "assigned task", launchMarker: MARKER });
eq(written.length, 1, "the task instructions were written once");
eq(written[0].indexOf("assigned task") !== -1, true, "the instructions keep the task");
eq(written[0].indexOf(MARKER) !== -1, true, "the instructions carry the supplied marker");

written.length = 0;
plugin.buildLaunchCommand!({ task: "assigned task" });
eq(written[0], "assigned task", "no marker leaves the instructions untouched");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
