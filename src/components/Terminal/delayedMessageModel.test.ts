import assert from "node:assert/strict";
import test from "node:test";
import { canSendDelayedMessage, sendCurrentDelayedMessage } from "./delayedMessageModel";
import type { DelayedMessage } from "./delayedSend";

test("missing and sending entries cannot be sent from a stale stash", async () => {
  for (const state of [undefined, "sending"] as const) {
    const message = state ? { id: "fired", state } as DelayedMessage : undefined;
    assert.equal(canSendDelayedMessage(message), false);
    await sendCurrentDelayedMessage("fired", { load: async () => message ? [message] : [], send: async () => assert.fail("stale entry sent") });
  }
});

test("a system heartbeat cannot be sent or replayed", async () => {
  const system = { id: "system:heartbeat:pl:tab", state: "queued", origin: "system" } as DelayedMessage;
  assert.equal(canSendDelayedMessage(system), false);
  assert.equal(canSendDelayedMessage({ ...system, origin: undefined }), false);
  await sendCurrentDelayedMessage(system.id, {
    load: async () => [system],
    send: async () => assert.fail("system entry sent"),
  });
});

test("send now uses the latest authoritative queued, stashed, or failed entry once", async () => {
  for (const state of ["queued", "stashed", "failed"] as const) {
    const sent: string[] = [];
    await sendCurrentDelayedMessage("current", { load: async () => [{ id: "current", state }] as DelayedMessage[], send: async (id) => { sent.push(id); } });
    assert.deepEqual(sent, ["current"]);
  }
});
