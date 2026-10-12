import assert from "node:assert/strict";
import test from "node:test";
import { retainedSendReceipt } from "./send-receipt.mjs";

test("retained user send continues transcript observation without another send", () => {
  const receipt = { ok: true, delivery: "pending", bytes_written: false, send_id: "own-send", status: "held" };
  assert.deepEqual(retainedSendReceipt(1, JSON.stringify({ ok: false, data: receipt })), receipt);
});

test("refusals, uncertain writes and unrelated failures do not authorize continuation", () => {
  for (const value of [
    { ok: false, error: "tab_busy_refused", bytes_written: false },
    { ok: true, delivery: "pending", bytes_written: true, send_id: "written" },
    { ok: true, delivery: "pending", bytes_written: false },
  ]) assert.equal(retainedSendReceipt(1, JSON.stringify({ data: value })), null);
  assert.equal(retainedSendReceipt(2, '{}'), null);
  assert.equal(retainedSendReceipt(1, 'broken'), null);
});
