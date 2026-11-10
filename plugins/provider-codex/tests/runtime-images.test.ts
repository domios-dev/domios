import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, Operation, RuntimeContext } from "@codeterm/plugin-sdk";

const sandbox = vm.createContext({ module: { exports: {} }, exports: {}, host: new Proxy({}, {
  get: () => { throw new Error("Image planning accessed host"); },
}) });
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), sandbox);
const hooks = sandbox.module.exports.default as AgentRuntime;
const plain = <T>(value: T): T => JSON.parse(JSON.stringify(value));
const context: RuntimeContext = { tabId: "images", cwd: "/work", hostVersion: "1.19.2", epoch: 1,
  bindingNonce: "nonce", threadId: "thread", settings: {}, providerSettings: {}, claimedSends: [], activeTurns: [], threadParents: [] };
let checks = 0;
for (const count of [1, 2]) {
  for (const inline of [false, true]) {
    const attachments = Array.from({ length: count }, (_, index) => ({ id: `image-${index}`, mimeType: "image/png",
      path: `/var/folders/temp/Снимок экрана ${index}\u202fPM.png`, ...(inline ? { data: "cGl4ZWxz" } : {}) }));
    const input = { text: "inspect", attachments };
    const expected = [{ type: "text", text: input.text }, ...attachments.map(image => inline
      ? { type: "image", url: "data:image/png;base64,cGl4ZWxz" } : { type: "localImage", path: image.path })];
    for (const busy of [false, true]) {
      const ctx = { ...context, ...(busy ? { activeTurnId: "active" } : {}) };
      const op: Operation = { kind: "submitTurn", threadId: "thread", sendId: "send", input, settings: {},
        ...(busy ? { activeTurnId: "active" } : {}) };
      const plan = plain(hooks.runtimePlan(ctx, op));
      assert.equal(plan.action.kind, "request");
      if (plan.action.kind !== "request") throw new Error("Request absent");
      assert.equal(plan.action.call.method, busy ? "turn/steer" : "turn/start");
      assert.deepEqual((plan.action.call.params as { input: unknown }).input, expected);
      if (busy) {
        const fallback = plain(hooks.runtimeResponse(ctx, op, plan.state, { kind: "error",
          error: { code: -32600, message: "no active turn to steer" } }));
        assert.equal(fallback.kind, "continue");
        if (fallback.kind !== "continue" || fallback.plan.action.kind !== "request") throw new Error("Fallback absent");
        assert.equal(fallback.beforeAcceptance, true);
        assert.equal(fallback.plan.action.call.method, "turn/start");
        assert.deepEqual((fallback.plan.action.call.params as { input: unknown }).input, expected);
      }
      checks++;
    }
  }
}
console.log(`runtime image start/steer/fallback: ${checks} checks passed`);
