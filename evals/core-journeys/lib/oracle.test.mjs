import assert from "node:assert/strict";
import test from "node:test";
import {
  attemptBindingVerdict, normalizeAnswer, pairTabs, readiness, replyVerdict, selectProviders, stopVerdict, taskVerdict,
} from "./oracle.mjs";

const provider = (id, extra = {}) => ({ id, availability: "available", ...extra });

test("selectProviders runs installed providers and fails a missing required one", () => {
  const { run, missing } = selectProviders([
    provider("claude"), provider("codex", { agent_runtime: { runtime: "stdio-jsonrpc" } }),
    provider("aider", { cli_status: { installed: false } }),
  ]);
  assert.deepEqual(run, [{ id: "claude", runtime: false }, { id: "codex", runtime: true }]);
  assert.deepEqual(missing.map((entry) => entry.id), ["opencode"]);
});

test("selectProviders with an explicit list ignores the required default", () => {
  const { run, missing } = selectProviders([provider("claude"), provider("grok", { availability: "unavailable" })], { only: ["claude", "grok"] });
  assert.deepEqual(run.map((entry) => entry.id), ["claude"]);
  assert.equal(missing[0].id, "grok");
});

test("readiness names terminal launch and runtime failures", () => {
  assert.match(readiness(null).fail, /absent/);
  assert.match(readiness({ boot_stage: "spawn_failed", boot_failure_reason: "desk leaf bound" }).fail, /desk leaf bound/);
  assert.match(readiness({ agent_type: "codex", boot_stage: "agent_ready", agent_session_id: "t",
    agent_runtime_state: { state: "degraded", reason: "SizeLimit" } }).fail, /SizeLimit/);
  assert.match(readiness({ agent_type: "claude", boot_stage: "agent_ready", pending_prompt: { question: "trust?" } }).fail, /owner prompt/);
});

test("readiness accepts a bound session and, before first send, a nonce-only lazy launch", () => {
  const ready = { agent_type: "claude", boot_stage: "agent_ready", agent_session_id: "s", status: "idle" };
  assert.equal(readiness(ready).done, ready);
  const lazy = { agent_type: "claude", boot_stage: "agent_ready", session_binding_nonce: "n" };
  assert.ok(readiness(lazy).done);
  assert.ok(readiness(lazy, { requireSession: true }).pending);
  assert.ok(readiness({ agent_type: "claude", boot_stage: "agent_launching" }).pending);
});

test("replyVerdict needs a new assistant item with the exact marker and idle", () => {
  const baselineIds = new Set(["old"]);
  const messages = [
    { id: "old", type: "assistant", content: "CJX" },
    { id: "u", type: "user", content: "Reply with exactly CJX" },
    { id: "a", type: "assistant", content: "`CJX`." },
  ];
  const working = { status: "working", agent_session_id: "s" };
  assert.ok(replyVerdict({ messages, baselineIds, marker: "CJX", record: working }).pending);
  const done = replyVerdict({ messages, baselineIds, marker: "CJX", record: { ...working, status: "idle" }, boundSession: "s" });
  assert.deepEqual(done.done, { answer: "CJX", session: "s" });
  assert.ok(replyVerdict({ messages: messages.slice(0, 2), baselineIds, marker: "CJX", record: { status: "idle" } }).pending);
});

test("replyVerdict fails when the tab changes session or the runtime degrades", () => {
  const record = { status: "idle", agent_session_id: "other" };
  assert.match(replyVerdict({ messages: [], baselineIds: new Set(), marker: "M", record, boundSession: "s" }).fail, /session changed/);
  const degraded = { status: "idle", agent_runtime_state: { state: "degraded", reason: "PipeEof" } };
  assert.match(replyVerdict({ messages: [], baselineIds: new Set(), marker: "M", record: degraded }).fail, /PipeEof/);
});

test("normalizeAnswer strips quoting and a trailing period only", () => {
  assert.equal(normalizeAnswer(" **CJ1**. "), "CJ1");
  assert.notEqual(normalizeAnswer("CJ1 done"), "CJ1");
});

test("taskVerdict tracks ACK, a report newer than the last one, and premature closure", () => {
  const attempt = { id: "a1", acknowledged_at: 5, reported_at: 10 };
  const task = { status: "in_review", execution_attempts: [attempt] };
  assert.ok(taskVerdict(task, { kind: "acked" }).done);
  assert.ok(taskVerdict(task, { kind: "reported" }).done);
  assert.ok(taskVerdict(task, { kind: "reported", after: 10 }).pending);
  assert.ok(taskVerdict({ ...task, status: "in_progress" }, { kind: "reported" }).pending);
  assert.match(taskVerdict({ ...task, status: "done" }, { kind: "reported" }).fail, /before review/);
  assert.ok(taskVerdict({ ...task, status: "done" }, { kind: "status", status: "done" }).done);
});

test("attemptBindingVerdict fails a restored Worker without its attempt_id", () => {
  const task = { status: "in_progress", execution_attempts: [{ id: "a1", executor_tab_id: "w" }] };
  assert.match(attemptBindingVerdict({ id: "w", task_binding: { plan_id: "p", task_id: "t" } }, task).fail, /no attempt_id/);
  assert.match(attemptBindingVerdict({ id: "w", task_binding: { attempt_id: "a0" } }, task).fail, /differs/);
  assert.match(attemptBindingVerdict({ id: "x", task_binding: { attempt_id: "a1" } }, task).fail, /executor/);
  assert.deepEqual(attemptBindingVerdict({ id: "w", task_binding: { attempt_id: "a1" } }, task).done, { attempt_id: "a1" });
});

test("pairTabs and stopVerdict read the API contracts", () => {
  assert.deepEqual(pairTabs({ orchestrator_tab_id: "o", planner_tab_id: "p", plan_id: "pl" }).done, { orchestrator: "o", planner: "p", plan_id: "pl" });
  assert.ok(pairTabs({ error: "leaf" }).fail);
  assert.match(stopVerdict({ stopped: false, reason: "stop_unsupported" }, { status: "idle" }).fail, /stop_unsupported/);
  assert.ok(stopVerdict({ stopped: true }, { status: "working" }).pending);
  assert.deepEqual(stopVerdict({ data: { stopped: true } }, { status: "idle", agent_session_id: "s" }).done, { session: "s" });
});
