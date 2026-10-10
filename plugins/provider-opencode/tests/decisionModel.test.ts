import assert from "node:assert/strict";
import { test } from "node:test";
import type { DecisionRequest } from "@codeterm/plugin-sdk";
import { decisionConfig, decisionModel, decisionPrompt, decisionRunOptions, parseDecisionEvents, validateDecision } from "../src/decisionModel";

const request: DecisionRequest = { instructions: "opaque", state: { key: "opaque-state" }, question: { type: "choice", options: { yes: "Yes", no: "No" } } };
const choice = { type: "choice", choice: "yes", probabilities: { yes: 0.8, no: 0.2 }, confidence: 0.8 };
const events = (answer: unknown, reason = "stop") => [
  { type: "step_start", part: {} },
  { type: "text", part: { text: JSON.stringify(answer) } },
  { type: "step_finish", part: { reason } },
].map((row) => JSON.stringify(row)).join("\n");

test("decision requests travel on stdin with a bounded deadline and tool-less agent", () => {
  const options = decisionRunOptions("fixture/model", request, 1200);
  assert.equal(options.timeoutMs, 1200);
  assert.deepEqual(JSON.parse(options.stdin!), JSON.parse(decisionPrompt(request)));
  assert.equal(options.args?.includes(options.stdin!), false);
  assert.deepEqual(options.args?.slice(0, 4), ["run", "--pure", "--format", "json"]);
  assert.equal(options.args?.includes("--auto"), false);
  const config = decisionConfig();
  assert.equal(config.permission["*"], "deny");
  assert.deepEqual(config.agent["domios-decision"].tools, { "*": false });
  for (const tool of ["bash", "edit", "external_directory", "question", "task", "skill"] as const) {
    assert.equal(config.agent["domios-decision"].permission[tool], "deny");
  }
  assert.equal(decisionRunOptions("fixture/model", request, 50000).timeoutMs, 4500);
});

test("complete JSON event streams map all shared decision shapes", () => {
  assert.deepEqual(parseDecisionEvents(events(choice), request.question), choice);
  const noul = { type: "noul", p: 0.8 };
  assert.deepEqual(parseDecisionEvents(events(noul), { type: "noul" }), noul);
  const score = { type: "score", score: 2, probabilities: { "0": 0.1, "1": 0.2, "2": 0.7 }, confidence: 0.7 };
  assert.deepEqual(parseDecisionEvents(events(score), { type: "score", levels: ["a", "b", "c"] }), score);
});

test("tool events and provider errors refuse a decision even alongside valid text", () => {
  for (const type of ["tool_use", "error"]) {
    assert.deepEqual(parseDecisionEvents(events(choice) + "\n" + JSON.stringify({ type }), request.question), { error: "transport" });
  }
});

test("incomplete malformed oversized and out-of-contract answers are typed failures", () => {
  for (const output of ["", "bad", events(choice, "length"), "x".repeat(1048577), events({ ...choice, choice: "foreign" }), events({ ...choice, probabilities: { yes: 2 } })]) {
    assert.deepEqual(parseDecisionEvents(output, request.question), { error: "invalid_response" });
  }
  assert.equal(validateDecision({ type: "noul", p: NaN }, { type: "noul" }), false);
  assert.equal(validateDecision({ type: "score", score: 9, probabilities: { "0": 1 }, confidence: null }, { type: "score", levels: ["a"] }), false);
});

test("adapter executes once and maps unavailable timeout and process errors without leaking details", () => {
  const scope = globalThis as { host?: unknown };
  const prior = scope.host;
  let calls = 0;
  let result: { code?: number; stdout?: string; error?: string } = {};
  const exec = Object.assign(() => JSON.stringify({ code: 0, stdout: "fixture/model" }), {
    async: <T>(opts: unknown, then: (value: typeof result) => T): T => {
      calls += 1;
      assert.equal(typeof opts, "object");
      return then(result);
    },
  });
  scope.host = { exec, fs: { readFile: () => null }, platform: () => "linux", envGet: () => null, expandHome: (path: string) => path };
  try {
    assert.deepEqual(decisionModel.decide(request, 1000), { error: "unavailable" });
    assert.equal(calls, 0);
    assert.equal(decisionModel.selectModel!("fixture/model"), true);
    for (const [response, expected] of [
      [{ error: "spawn failed: opaque remote detail" }, { error: "unavailable" }],
      [{ error: "opencode timed out after 1000ms" }, { error: "timeout" }],
      [{ code: 1, stdout: "opaque remote detail" }, { error: "transport" }],
      [{ code: 0, stdout: events(choice) }, choice],
    ] as const) {
      result = response;
      const previous = calls;
      assert.deepEqual(decisionModel.decide(request, 1000), expected);
      assert.equal(calls, previous + 1);
    }
    const previous = calls;
    assert.deepEqual(decisionModel.decide(request, 0), { error: "timeout" });
    assert.equal(calls, previous);
  } finally {
    scope.host = prior;
  }
});
