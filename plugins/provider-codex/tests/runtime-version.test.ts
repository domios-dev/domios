import assert from "node:assert/strict";
import { runtimePlan, runtimeResponse } from "../src/runtime";
import { runtimeBinaryVersion } from "../src/runtimeVersion";
import type { RuntimeContext, RuntimeStep, JsonValue, RpcReply } from "@codeterm/plugin-sdk";

const ctx = { hostVersion: "fixture", cwd: "/fixture" } as RuntimeContext;
const operation = { kind: "initialize" } as const;
function next(step: RuntimeStep, reply: RpcReply): RuntimeStep {
  assert.equal(step.kind, "continue");
  if (step.kind !== "continue") throw new Error("Expected continuation");
  return runtimeResponse(ctx, operation, step.plan.state, reply);
}
function handshake(account: JsonValue, userAgent: unknown = "codex_cli_rs/0.163.0 (fixture)") {
  const plan = runtimePlan(ctx, operation);
  let step = runtimeResponse(ctx, operation, plan.state, { kind: "success", value: { userAgent } as JsonValue });
  step = next(step, { kind: "success", value: null });
  return next(step, { kind: "success", value: account });
}
function version(step: RuntimeStep) {
  assert.equal(step.kind, "complete");
  return step.kind === "complete" ? (step.result as { binaryVersion?: string }).binaryVersion : undefined;
}
assert.equal(version(handshake({ account: { type: "apiKey" } })), "0.163.0");
assert.equal(version(handshake({ account: null, requiresOpenaiAuth: true })), "0.163.0");
const subscription = () => handshake({ account: { type: "chatgpt", email: "fixture@example.test", planType: "plus" } });
assert.equal(version(next(subscription(), { kind: "success", value: { rateLimits: {} } })), "0.163.0");
assert.equal(version(next(subscription(), { kind: "error", error: { code: -32000, message: "fixture failure" } })), "0.163.0");
assert.equal(version(handshake({ account: { type: "apiKey" } }, "unknown")), undefined);
for (const input of [null, "", "provider/0.163.0-beta", "unrelated text 0.163.0", "provider/01.2.3"]) {
  assert.equal(runtimeBinaryVersion(input), undefined);
}
console.log("runtime-version: 10 cases passed");
