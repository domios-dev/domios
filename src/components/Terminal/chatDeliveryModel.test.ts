import assert from "node:assert/strict";
import { test } from "node:test";
import { applyDeliveryStatus, deliveryState, sendForRow, reconcileDeliveryStatusesModel, reconcileAdmissionEvidence, deliveryProjectionSends, type DeliverySend } from "./chatDeliveryModel";
import { createTranscriptState, reduceTranscript, projectTranscript } from "@codeterm/chat-core";
import type { ChatSendStatus } from "./chatSendStatus";

const send = (): DeliverySend => ({ id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1000 });
const status = (state: ChatSendStatus["state"], sequence: number): ChatSendStatus => ({
  tab_id: "tab", send_id: "transaction", state, sequence, ledger_id: "ledger",
  retained_from: 1, error: null, correlationId: "token",
});

test("sending becomes delivered without changing the bubble", () => {
  const initial = send();
  assert.equal(deliveryState(initial), "sending");
  const written = applyDeliveryStatus(initial, status("written", 2));
  assert.equal(deliveryState(written), "sending");
  const delivered = applyDeliveryStatus(written, status("delivered", 3));
  assert.equal(deliveryState(delivered), "delivered");
  assert.equal(delivered.id, "bubble");
});

test("held queues and exact evidence delivers the same bubble", () => {
  const queued = applyDeliveryStatus(send(), status("held", 1));
  assert.equal(deliveryState(queued), "queued");
  const delivered = applyDeliveryStatus(queued, status("delivered", 4));
  assert.equal(deliveryState(delivered), "delivered");
  assert.equal(delivered.id, queued.id);
});

test("uncertain writes show uncertainty and stale responses cannot downgrade evidence", () => {
  const uncertain = applyDeliveryStatus(send(), status("submit_unconfirmed", 3));
  assert.equal(deliveryState(uncertain), "uncertain");
  assert.equal(applyDeliveryStatus(uncertain, status("held", 1)), uncertain);
  const delivered = applyDeliveryStatus(uncertain, status("delivered", 4));
  assert.equal(applyDeliveryStatus(delivered, status("failed", 5)), delivered);
});

test("Retry's fresh transaction ignores the old failed token", () => {
  const failed = applyDeliveryStatus(send(), status("failed", 4));
  assert.equal(deliveryState(failed), "failed");
  const retry = { ...failed, status: undefined, correlationId: "retry-token" };
  assert.equal(deliveryState(retry), "sending");
  assert.equal(applyDeliveryStatus(retry, status("failed", 5)), retry);
  const delivered = applyDeliveryStatus(retry, { ...status("delivered", 6), send_id: "retry-transaction", correlationId: "retry-token" });
  assert.equal(deliveryState(delivered), "delivered");
  assert.equal(delivered.id, "bubble");
});

test("source echo resolves to the optimistic send by correlation or recorded identity", () => {
  const row = { id: "source", type: "user" as const, content: "opaque", timestamp: "1000", correlationId: "token" };
  const initial = send();
  assert.equal(sendForRow([initial], row), initial);
  const recorded = { ...initial, echo: { content: "opaque", timestampMs: 1000000 } };
  assert.equal(sendForRow([recorded], { ...row, correlationId: undefined }), recorded);
  assert.equal(sendForRow([recorded], { ...row, type: "assistant" }), undefined);
});

test("failure receipt overrides a speculative echo and late held replies cannot undo it", () => {
  const echoed = { ...send(), echo: { content: "opaque", timestampMs: 1000 } };
  const failed = applyDeliveryStatus(echoed, status("failed", 1));
  assert.equal(deliveryState(failed), "failed");
  assert.equal(failed.echo, undefined);
  assert.equal(applyDeliveryStatus(failed, status("held", 1)), failed);
  const delivered = applyDeliveryStatus(failed, status("delivered", 1));
  assert.equal(deliveryState(delivered), "delivered");
});

test("a snapshot started before settlement cannot erase the live failure receipts", () => {
  const requested = [send(), { ...send(), id: "second", correlationId: "second-token" }];
  const failed = requested.map((item) => applyDeliveryStatus(item, {
    ...status("failed", 1), correlationId: item.correlationId, send_id: item.id,
  }));
  const result = reconcileDeliveryStatusesModel(failed, [], requested);
  assert.deepEqual(result, failed);
  assert(result.every((item) => deliveryState(item) === "failed"));
  const current = reconcileDeliveryStatusesModel(failed, [], failed);
  assert(current.every((item) => item.status == null));
  const echoed = requested.map((item) => ({ ...item, echo: { content: item.content, timestampMs: item.sentAtMs } }));
  const restored = reconcileDeliveryStatusesModel(echoed, failed.map((item) => item.status!), requested);
  assert(restored.every((item) => deliveryState(item) === "failed" && item.echo == null));
});

test("the first failure snapshot applies after an in-flight held response changes both sends", () => {
  const requested = [send(), { ...send(), id: "second", correlationId: "second-token" }];
  const held = requested.map((item) => applyDeliveryStatus(item, {
    ...status("held", 1), correlationId: item.correlationId, send_id: item.id,
  }));
  const receipts = held.map((item) => ({ ...item.status!, state: "failed" as const }));
  const failed = reconcileDeliveryStatusesModel(held, receipts, requested);
  assert(failed.every((item) => deliveryState(item) === "failed"));
  assert.deepEqual(failed.map((item) => item.id), requested.map((item) => item.id));
  const lateHeld = reconcileDeliveryStatusesModel(failed, held.map((item) => item.status!), requested);
  assert(lateHeld.every((item) => deliveryState(item) === "failed"));
});

test("reloaded admissions adopt failed or delivered outcomes from the restarted ledger", () => {
  for (const outcome of ["failed", "delivered"] as const) {
    const reloaded = JSON.parse(JSON.stringify([applyDeliveryStatus(send(), status("held", 10))])) as DeliverySend[];
    const receipt = { ...status(outcome, 1), ledger_id: "restarted-ledger" };
    const result = reconcileDeliveryStatusesModel(reloaded, [receipt]);
    assert.equal(result[0].status?.state, outcome);
    assert.equal(deliveryState(result[0]), outcome);
    assert.equal(result[0].id, reloaded[0].id);
  }
});

test("interrupted admissions leave sending without turning ambiguous responses into Retry", () => {
  for (const requestHadResponse of [false, true, undefined]) {
    for (const evidence of ["present", "absent", "unknown"] as const) {
      const reloaded = JSON.parse(JSON.stringify({ ...send(), requestHadResponse })) as DeliverySend;
      const result = reconcileAdmissionEvidence(reloaded, evidence);
      assert.equal(deliveryState(result), evidence === "absent" && requestHadResponse === false ? "failed" : "uncertain");
    }
  }
  for (const state of ["held", "written", "submit_unconfirmed", "delivered", "failed"] as const) {
    const known = applyDeliveryStatus(send(), status(state, 1));
    assert.equal(reconcileAdmissionEvidence(known, "absent"), known);
  }
});

test("two failed sends remain visible across reload alongside a similar new session turn", () => {
  const content = (token: string) => `shared alpha beta gamma delta ${token}`;
  const failed = ["first", "second"].map((token, index) => applyDeliveryStatus({
    id: token, correlationId: token, content: content(token), sentAtMs: 1700000000000 + index,
  }, { ...status("failed", index + 1), correlationId: token, send_id: token }));
  const transcript = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority: { sourceSessionId: "new-session", sessionBindingNonce: "new-nonce" },
    range: { start: 0, end: 1 }, rows: [{ id: "new-turn", type: "user",
      content: content("third"), timestamp: "1700000001000" }],
  }).state;
  for (const sends of [failed, JSON.parse(JSON.stringify(failed)) as DeliverySend[]]) {
    const projected = projectTranscript(transcript, deliveryProjectionSends(sends, 1700000001000));
    assert.deepEqual(projected.pending.map((item) => item.id), ["first", "second"]);
    assert.equal(projected.echoes.size, 0);
    assert.equal(projected.rows.length, 3);
    for (const id of ["first", "second"]) {
      const row = projected.rows.find((row) => row.id === id)!;
      assert.equal(deliveryState(sendForRow(sends, row)!), "failed");
    }
  }
});

test("written sends flow above later rows while held and failed sends retain their state", () => {
  const at = 1_800_000_000_000;
  const transcript = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority: { sourceSessionId: "own", sessionBindingNonce: "launch" }, range: null,
    rows: [
      { id: "old", type: "assistant", content: "earlier", timestamp: String(at - 1000) },
      { id: "new", type: "assistant", content: "later", timestamp: String(at + 1000) },
    ],
  }).state;
  for (const state of ["written", "submit_unconfirmed", "delivered", "held", "failed"] as const) {
    const current = applyDeliveryStatus({ ...send(), sentAtMs: at }, status(state, 2));
    const projected = projectTranscript(transcript, deliveryProjectionSends([current]));
    assert.deepEqual(projected.rows.map((row) => row.id),
      ["held", "failed"].includes(state) ? ["old", "new", "bubble"] : ["old", "bubble", "new"]);
    assert.equal(projected.pending.length, 1);
    assert.equal(deliveryState(current), state === "written" ? "sending" : state === "submit_unconfirmed" ? "uncertain"
      : state === "held" ? "queued" : state);
  }
});
