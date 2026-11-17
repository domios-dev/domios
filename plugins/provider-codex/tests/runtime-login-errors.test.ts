import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, Operation, RuntimeContext } from "@codeterm/plugin-sdk";

const sandbox = vm.createContext({ module: { exports: {} }, exports: {}, host: {} });
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), sandbox);
const runtime = sandbox.module.exports.default as AgentRuntime;
const context: RuntimeContext = { epoch: 1, tabId: "tab", bindingNonce: "nonce", cwd: "/fixture", hostVersion: "1.20.1", claimedSends: [], activeTurns: [], threadParents: [], settings: {}, providerSettings: {} };
const plain = (value: unknown) => JSON.parse(JSON.stringify(value));

let checks = 0;
function check(name: string, run: () => void) { run(); checks++; console.log(`ok ${name}`); }

check("a refused login start surfaces the app-server reason without links", () => {
  const operation: Operation = { kind: "control", action: "login", value: { method: "browser" } };
  const plan = runtime.runtimePlan(context, operation);
  const step = runtime.runtimeResponse(context, operation, plan.state, { kind: "error", error: { code: -32600, message: "failed to start login server: port https://auth.example.test/x?state=opaque in use" } });
  assert.equal(step.kind, "refused");
  if (step.kind === "refused") assert.equal(step.reason.message, "Sign-in failed: failed to start login server: port <link> in use");
});

check("an empty login error keeps the generic reason", () => {
  const operation: Operation = { kind: "control", action: "login", value: { method: "device" } };
  const plan = runtime.runtimePlan(context, operation);
  const step = runtime.runtimeResponse(context, operation, plan.state, { kind: "error", error: { code: -32603, message: "  " } });
  if (step.kind !== "refused") assert.fail("Login error was not refused");
  assert.equal(step.reason.message, "Sign-in failed. Try again.");
});

check("a failed completion carries the bounded app-server error", () => {
  const events = runtime.runtimeReduce(context, null, [{ kind: "notification", method: "account/login/completed", params: { loginId: "own", success: false, error: `Login was not completed ${"x".repeat(400)}`, onboardingEntrypoint: null } }]).events;
  const login = plain(events)[0].login;
  assert.equal(login.status, "failed");
  assert.equal(login.loginId, "own");
  assert.ok(login.error.startsWith("Sign-in did not complete: Login was not completed"));
  assert.ok(login.error.length <= "Sign-in did not complete: ".length + 240);
});

check("the app-server's lowercase apikey auth mode maps to API billing", () => {
  const events = runtime.runtimeReduce(context, null, [{ kind: "notification", method: "account/updated", params: { authMode: "apikey", planType: null } }]).events;
  assert.deepEqual(plain(events), [{ kind: "auth", state: "signedIn", billingMode: "api" }]);
});

console.log(`runtime login error regressions: ${checks} checks passed`);
