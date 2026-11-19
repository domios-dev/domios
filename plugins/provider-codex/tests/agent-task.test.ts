import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import type { AgentTaskInput, AgentTaskResult, HostIoSteps } from "@codeterm/plugin-sdk";

const module = { exports: {} };
let now = 10_000;
vm.runInNewContext(readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), {
  module,
  host: {
    homeDir: () => "/fixture-home",
    unixNowMs: () => now,
    io: { run: (steps: HostIoSteps<unknown>) => steps },
  },
});
const plugin = (module.exports as { default: {
  runAgentTask(input: AgentTaskInput): HostIoSteps<AgentTaskResult>;
  abortAgentTask(runId: string): HostIoSteps<null>;
} }).default;

const input: AgentTaskInput = {
  runId: "pipeline-run",
  tabId: "creator-tab",
  workspaceId: "creator-workspace",
  profile: "gpt-6.2-sol",
  task: "opaque instruction",
  input: "opaque input",
  timeoutMs: 5_000,
  maxTokens: 1_000,
  maxTurns: 1,
};
const plain = (value: unknown) => JSON.parse(JSON.stringify(value));
const usage = { input_tokens: 600, cached_input_tokens: 500, output_tokens: 20 };
function receipt(threadId: string, text = "opaque answer") {
  return {
    done: true,
    code: 0,
    stdout: [
      { type: "thread.started", thread_id: threadId },
      { type: "item.completed", item: { type: "agent_message", text: "Earlier commentary" } },
      { type: "item.completed", item: { type: "agent_message", text } },
      { type: "turn.completed", usage },
    ].map(event => JSON.stringify(event)).join("\n"),
  };
}
function start(runId = input.runId, maxTokens = input.maxTokens) {
  const run = plugin.runAgentTask({ ...input, runId, maxTokens });
  const effect = run.next().value;
  assert(effect && "op" in effect && effect.op === "execStart");
  const options = plain(effect.options);
  assert.equal(options.bin, "codex");
  assert.equal(options.stdin, input.task + "\n\n" + input.input);
  assert.equal(options.timeoutMs, input.timeoutMs);
  assert(options.args.includes("--ephemeral"));
  assert(options.args.includes("read-only"));
  assert.equal(options.args[options.args.indexOf("--model") + 1], input.profile);
  assert(!JSON.stringify(options).includes(input.tabId));
  return run;
}
function attach(run: HostIoSteps<AgentTaskResult>, job: string) {
  assert.deepEqual(plain(run.next({ jobId: job })), {
    done: false, value: { op: "awaitJob", job },
  });
}

const success = start();
attach(success, "exec-first");
assert.deepEqual(plain(success.next(receipt("thread-first")).value), {
  status: "done", output: { text: "opaque answer", threadId: "thread-first", usage },
});

const review = start();
attach(review, "exec-review");
assert.deepEqual(plain(review.next(receipt("thread-review", '{"verdict":"pass"}')).value), {
  status: "done", output: { verdict: "pass" },
});

const cancelled = start();
attach(cancelled, "exec-cancelled");
const concurrent = start("other-run");
attach(concurrent, "exec-other");
const abort = plugin.abortAgentTask(input.runId);
assert.deepEqual(plain(abort.next().value), { op: "execClose", job: "exec-cancelled" });
assert.deepEqual(plain(abort.next()), { done: true, value: null });
assert.deepEqual(plain(cancelled.next(receipt("thread-cancelled")).value), {
  status: "cancelled", output: null,
});
assert.equal(plain(concurrent.next(receipt("thread-other")).value).output.threadId, "thread-other");
assert.deepEqual(plain(plugin.abortAgentTask(input.runId).next()), { done: true, value: null });

const starting = start();
assert.deepEqual(plain(plugin.abortAgentTask(input.runId).next()), { done: true, value: null });
assert.deepEqual(plain(starting.next({ jobId: "exec-late" }).value), { op: "execClose", job: "exec-late" });
assert.deepEqual(plain(starting.next().value), { status: "cancelled", output: null });

const expired = start();
attach(expired, "exec-expired");
now += input.timeoutMs;
assert.deepEqual(plain(expired.next(receipt("thread-expired")).value), {
  status: "deadline_exceeded", output: null,
});

const timedOut = start();
attach(timedOut, "exec-native-timeout");
assert.deepEqual(plain(timedOut.next({ done: true, error: "codex timed out after 5000ms" }).value), {
  status: "deadline_exceeded", output: null,
});

const overBudget = start(input.runId, 619);
attach(overBudget, "exec-over-budget");
assert.deepEqual(plain(overBudget.next(receipt("thread-over-budget")).value), {
  status: "failed", output: null, error: "Codex task token budget exceeded",
});

const incomplete = start();
attach(incomplete, "exec-incomplete");
const partial = receipt("thread-incomplete");
partial.stdout = partial.stdout.split("\n").slice(0, -1).join("\n");
assert.equal(plain(incomplete.next(partial).value).status, "failed");
console.log("Codex agent-task fixtures passed");
