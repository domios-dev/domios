// Run: npx tsx packages/chat-core/src/shellCommands.test.ts
import {
  applyShellLifecycle,
  shellSnapshotToMessages,
  type CommandLifecycle,
} from "./shellCommands";
import type { NormalizedChatMessage } from "./types";

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  const match = JSON.stringify(actual) === JSON.stringify(expected);
  if (match) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}\n    expected ${JSON.stringify(expected)}\n    actual   ${JSON.stringify(actual)}`);
  }
}

function ok(cond: unknown, name: string) {
  if (cond) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}`);
  }
}

const live = (i: number) => ({ id: `live-${i}`, timestampMs: 2_000 + i });

// snapshot hydration → finished items, ordered, monotonic timestamps
{
  const msgs = shellSnapshotToMessages(
    {
      commands: [
        { commandLine: "ls -la", exitCode: 0, cwd: "/home", hasOutputStart: true },
        { commandLine: "false", exitCode: 1, cwd: "/home", hasOutputStart: false },
      ],
      cwd: "/home",
    },
    "snap",
    1_000,
  );
  eq(msgs.length, 2, "snapshot yields one item per command");
  eq(msgs[0].type, "shell_command", "snapshot item is shell_command kind");
  eq(msgs[0].shellStatus, "ok", "exit 0 → ok");
  eq(msgs[1].shellStatus, "failed", "exit 1 → failed");
  eq(msgs[1].shellExitCode, 1, "failed item carries exit code");
  eq(msgs[1].shellHasOutputStart, false, "A/D-only command preserves has_output_start=false");
  ok(Number(msgs[0].timestamp) < Number(msgs[1].timestamp), "timestamps are monotonic by order");
  eq(msgs[0].content, "ls -la", "content mirrors command line");
}

// live lifecycle: started → running, then finished updates in place (same id)
{
  let msgs: NormalizedChatMessage[] = [];
  msgs = applyShellLifecycle(msgs, { kind: "started", commandLine: "npm test", cwd: "/repo" }, live(0));
  eq(msgs.length, 1, "started appends a running item");
  eq(msgs[0].shellStatus, "running", "started → running");
  eq(msgs[0].shellHasOutputStart, true, "started implies output-start");
  const runningId = msgs[0].id;

  msgs = applyShellLifecycle(msgs, { kind: "finished", commandLine: "npm test", exitCode: 0, cwd: "/repo" }, live(1));
  eq(msgs.length, 1, "finished updates in place — no new item");
  eq(msgs[0].id, runningId, "finished keeps the running item's id (stable bubble)");
  eq(msgs[0].shellStatus, "ok", "finished exit 0 → ok");
}

// failed exit code
{
  let msgs: NormalizedChatMessage[] = [];
  msgs = applyShellLifecycle(msgs, { kind: "started", commandLine: "boom" }, live(0));
  msgs = applyShellLifecycle(msgs, { kind: "finished", commandLine: "boom", exitCode: 127 }, live(1));
  eq(msgs[0].shellStatus, "failed", "non-zero exit → failed");
  eq(msgs[0].shellExitCode, 127, "exit code surfaced");
}

// A/D-only graceful degradation: finished with NO preceding started
{
  let msgs: NormalizedChatMessage[] = [];
  msgs = applyShellLifecycle(msgs, { kind: "finished", commandLine: "git status", exitCode: 0, cwd: "/x" }, live(0));
  eq(msgs.length, 1, "finished with no started still produces an item");
  eq(msgs[0].shellStatus, "ok", "A/D-only finished resolves status");
  eq(msgs[0].shellHasOutputStart, false, "A/D-only item has_output_start=false");
  eq(msgs[0].shellCwd, "/x", "A/D-only item carries cwd");
}

// cwdChanged is informational — no chat item, same array reference
{
  const before: NormalizedChatMessage[] = [];
  const after = applyShellLifecycle(before, { kind: "cwdChanged", cwd: "/new" }, live(0));
  ok(after === before, "cwdChanged returns the same array (no spurious item)");
}

// two sequential commands stay distinct
{
  let msgs: NormalizedChatMessage[] = [];
  msgs = applyShellLifecycle(msgs, { kind: "started", commandLine: "a" }, live(0));
  msgs = applyShellLifecycle(msgs, { kind: "finished", commandLine: "a", exitCode: 0 }, live(1));
  msgs = applyShellLifecycle(msgs, { kind: "started", commandLine: "b" }, live(2));
  msgs = applyShellLifecycle(msgs, { kind: "finished", commandLine: "b", exitCode: 0 }, live(3));
  eq(msgs.length, 2, "two commands → two items");
  eq([msgs[0].shellCommand, msgs[1].shellCommand], ["a", "b"], "order preserved");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
