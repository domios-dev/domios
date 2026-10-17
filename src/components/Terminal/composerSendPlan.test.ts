import assert from "node:assert/strict";
import test from "node:test";
import { composerSendPlan, resolveComposerSend } from "./composerSendPlan";
import { capsuleToken, serializeCapsuleDraft, type CapsuleRegistry } from "./composerCapsules";

const registry: CapsuleRegistry = {
  pending: { label: "pending", kind: "image", path: "", uploadId: 1 },
  failed: { label: "failed", kind: "image", path: "", uploadId: 2 },
  ready: { label: "ready", kind: "image", path: "/ready.png" },
};
test("pending uploads belong to the send while the composer clears immediately", () => {
  assert.deepEqual(composerSendPlan(capsuleToken("pending"), registry, [{ id: 1, state: "uploading" }]), { clearComposer: true, uploadIds: [1], waitForUploadIds: [1], dropUploadIds: [] });
});
test("failed uploads are excluded while text remains sendable", () => {
  const result = resolveComposerSend(`hello ${capsuleToken("failed")}`, registry, new Map([[2, null]]));
  assert.equal(result.text, "hello");
  assert.equal(result.capsules.failed, undefined);
});
test("mixed uploads preserve ready references and positions, excluding failures", () => {
  const result = resolveComposerSend(`left ${capsuleToken("pending")} middle ${capsuleToken("failed")} ${capsuleToken("ready")} right`, registry, new Map([[1, "/resolved.png"], [2, null]]));
  const portable = serializeCapsuleDraft(result.text, result.capsules);
  assert(portable.indexOf("/resolved.png") > portable.indexOf("left"));
  assert(portable.indexOf("/resolved.png") < portable.indexOf("middle"));
  assert(portable.includes("/ready.png"));
  assert(!portable.includes("failed"));
});

test("failed chips do not delay the send, mixed chips wait only for pending uploads", () => {
  const states = [{ id: 1, state: "uploading" }, { id: 2, state: "failed" }] as const;
  assert.deepEqual(composerSendPlan(capsuleToken("failed"), registry, states),
    { clearComposer: true, uploadIds: [2], waitForUploadIds: [], dropUploadIds: [2] });
  assert.deepEqual(composerSendPlan(capsuleToken("pending") + capsuleToken("failed"), registry, states),
    { clearComposer: true, uploadIds: [1, 2], waitForUploadIds: [1], dropUploadIds: [2] });
});
