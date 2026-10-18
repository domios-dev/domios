import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import {
  addChatSend, beginChatSendRetry, failChatSendRequest, forgetChatSendsInMemory, removeChatSend,
  observeChatSendDismissal, observeDeliveryStatus, readChatSends, reconcileChatSendAdmission,
  reconcileDeliveryStatuses, recordChatSendAdmissionFailure, setChatSendDraft, useChatSends,
} from "./chatSendStore";
import { deliveryState } from "./chatDeliveryModel";
import type { ChatSendStatus } from "./chatSendStatus";

test("Retry preserves the row, guards repeated clicks, and accepts only the fresh transaction", () => {
  const tabId = "retry-store";
  addChatSend(tabId, { id: "bubble", correlationId: "original-token", content: "opaque", sentAtMs: 1700000000000 });
  const status: ChatSendStatus = { tab_id: tabId, send_id: "first", state: "failed", ledger_id: "ledger",
    sequence: 4, retained_from: 1, error: null, correlationId: "original-token" };
  observeDeliveryStatus(status);
  const retry = beginChatSendRetry(tabId, "bubble");
  assert(retry);
  assert.notEqual(retry.correlationId, "original-token");
  assert.equal(retry.id, "bubble");
  assert.equal(retry.sentAtMs, 1700000000000);
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
  observeDeliveryStatus({ ...status, sequence: 5 });
  assert.equal(deliveryState(readChatSends(tabId)[0]), "sending");
  observeDeliveryStatus({ ...status, send_id: "second", sequence: 6, state: "delivered", correlationId: retry.correlationId });
  assert.equal(readChatSends(tabId).length, 1);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "delivered");
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
});

async function withStoreProbe(
  tabId: string,
  run: (probe: { writes: string[]; serializedBytes: number[]; renderCount: () => number }) => Promise<void>,
): Promise<void> {
  const storage = window.localStorage;
  const originalSetItem = storage.setItem.bind(storage);
  const writes: string[] = [];
  const serializedBytes: number[] = [];
  Object.defineProperty(storage, "setItem", {
    configurable: true,
    value: (key: string, value: string) => {
      writes.push(key);
      serializedBytes.push(Buffer.byteLength(value, "utf8"));
      originalSetItem(key, value);
    },
  });

  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let renders = 0;
  function Probe() {
    useChatSends(tabId);
    renders += 1;
    return null;
  }

  try {
    await act(async () => root.render(createElement(Probe)));
    await run({ writes, serializedBytes, renderCount: () => renders });
  } finally {
    await act(async () => root.unmount());
    host.remove();
    Object.defineProperty(storage, "setItem", { configurable: true, value: originalSetItem });
  }
}

test("replayed dismissal receipts do not republish and dismissal survives reload", async () => {
  const tabId = "idempotent-dismissal";
  const dismissed = { id: "dismissed", correlationId: "dismiss-token", content: "opaque", sentAtMs: 10 };
  const receipt = { send_id: "receipt-1", send: dismissed };
  await withStoreProbe(tabId, async ({ writes, renderCount }) => {
    await act(async () => addChatSend(tabId, dismissed));

    const beforeDismissal = { writes: writes.length, renders: renderCount() };
    await act(async () => observeChatSendDismissal(tabId, receipt));
    assert.equal(writes.length, beforeDismissal.writes + 1);
    assert.equal(renderCount(), beforeDismissal.renders + 1);

    const afterFirstDismissal = { writes: writes.length, renders: renderCount() };
    for (let index = 0; index < 4; index += 1) {
      await act(async () => observeChatSendDismissal(tabId, receipt));
    }
    assert.equal(writes.length, afterFirstDismissal.writes);
    assert.equal(renderCount(), afterFirstDismissal.renders);

    await act(async () => forgetChatSendsInMemory());
    assert.equal(readChatSends(tabId).find((send) => send.id === dismissed.id)?.dismissed, true);
  });
});

test("unchanged admission evidence does not republish while changed evidence survives reload", async () => {
  const tabId = "idempotent-admission";
  const admission = { id: "admission", correlationId: "admission-token", content: "opaque", sentAtMs: 11 };
  await withStoreProbe(tabId, async ({ writes, renderCount }) => {
    await act(async () => addChatSend(tabId, admission));

    const beforeUnknown = { writes: writes.length, renders: renderCount() };
    await act(async () => reconcileChatSendAdmission(tabId, admission.correlationId, "unknown"));
    assert.equal(writes.length, beforeUnknown.writes + 1);
    assert.equal(renderCount(), beforeUnknown.renders + 1);

    const afterFirstUnknown = { writes: writes.length, renders: renderCount() };
    for (let index = 0; index < 4; index += 1) {
      await act(async () => reconcileChatSendAdmission(tabId, admission.correlationId, "unknown"));
    }
    assert.equal(writes.length, afterFirstUnknown.writes);
    assert.equal(renderCount(), afterFirstUnknown.renders);

    const beforeFailureEvidence = { writes: writes.length, renders: renderCount() };
    await act(async () => recordChatSendAdmissionFailure(tabId, admission.correlationId, false));
    assert.equal(writes.length, beforeFailureEvidence.writes + 1);
    assert.equal(renderCount(), beforeFailureEvidence.renders + 1);

    const afterFailureEvidence = { writes: writes.length, renders: renderCount() };
    for (let index = 0; index < 4; index += 1) {
      await act(async () => recordChatSendAdmissionFailure(tabId, admission.correlationId, false));
    }
    assert.equal(writes.length, afterFailureEvidence.writes);
    assert.equal(renderCount(), afterFailureEvidence.renders);

    const beforeAbsent = { writes: writes.length, renders: renderCount() };
    await act(async () => reconcileChatSendAdmission(tabId, admission.correlationId, "absent"));
    assert.equal(writes.length, beforeAbsent.writes + 1);
    assert.equal(renderCount(), beforeAbsent.renders + 1);

    await act(async () => forgetChatSendsInMemory());
    const restored = readChatSends(tabId).find((send) => send.id === admission.id);
    assert.equal(deliveryState(restored!), "failed");
  });
});

test("synthetic multi-tab ledger records one bounded write size for a changed dismissal", async (t) => {
  const rawSends = Object.fromEntries(Array.from({ length: 32 }, (_, tabIndex) => {
    const tabId = `synthetic-tab-${tabIndex}`;
    return [tabId, Array.from({ length: 32 }, (_, sendIndex) => {
      const payload = `synthetic-${tabIndex}-${sendIndex}`.padEnd(512, "x");
      return {
        id: `synthetic-send-${sendIndex}`,
        correlationId: `synthetic-token-${sendIndex}`,
        content: payload,
        sentAtMs: 100 + sendIndex,
        echo: { content: payload, timestampMs: 100 + sendIndex },
        status: { state: "written", system_send: { content: payload, sent_at_ms: 100 + sendIndex } },
        draft: { text: payload, attachments: [], recordedVoiceAttachments: [], capsules: {}, reply: null },
      };
    })];
  }));
  const storage = window.localStorage;
  storage.setItem("codeterm:chat-sends:v1", JSON.stringify(rawSends));
  forgetChatSendsInMemory();

  const tabId = "synthetic-tab-0";
  const receipt = {
    send_id: "synthetic-receipt",
    send: { id: "synthetic-send-0", correlationId: "synthetic-token-0", content: "opaque", sentAtMs: 100 },
  };
  try {
    await withStoreProbe(tabId, async ({ writes, serializedBytes, renderCount }) => {
      const before = renderCount();
      await act(async () => observeChatSendDismissal(tabId, receipt));
      assert.equal(writes.length, 1);
      assert.equal(renderCount(), before + 1);
      assert(serializedBytes[0] > 1_000_000);

      for (let index = 0; index < 4; index += 1) {
        await act(async () => observeChatSendDismissal(tabId, receipt));
      }
      assert.equal(writes.length, 1);
      assert.equal(renderCount(), before + 1);
      t.diagnostic(`synthetic multi-tab ledger: 32 tabs, 1,024 records; writes=1; serialized UTF-8 bytes=${serializedBytes[0]}`);
    });
  } finally {
    forgetChatSendsInMemory();
    storage.removeItem("codeterm:chat-sends:v1");
  }
});

test("voice cancellation removes only its exact optimistic row", () => {
  const tabId = "voice-cancel-store";
  addChatSend(tabId, { id: "voice-row", correlationId: "voice-token", content: "voice", sentAtMs: 10 });
  addChatSend(tabId, { id: "other-row", correlationId: "other-token", content: "other", sentAtMs: 11 });
  removeChatSend(tabId, "voice-token");
  assert.deepEqual(readChatSends(tabId).map((send) => send.id), ["other-row"]);
});

test("unconfirmed writes never offer Retry even after a late request rejection", () => {
  const tabId = "uncertain-store";
  addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1700000000000 });
  observeDeliveryStatus({ tab_id: tabId, send_id: "tx", correlationId: "token", state: "submit_unconfirmed",
    error: null, ledger_id: "ledger", sequence: 3, retained_from: 1 });
  failChatSendRequest(tabId, "token");
  assert.equal(deliveryState(readChatSends(tabId)[0]), "uncertain");
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
});

test("unprepared attachment sends survive restart on their original bubble", () => {
  const tabId = "restored-preparation";
  addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1700000000000 });
  setChatSendDraft(tabId, "bubble", { text: "opaque", attachments: ["/opaque/file"], recordedVoiceAttachments: [], capsules: {}, reply: null });
  failChatSendRequest(tabId, "bubble");
  forgetChatSendsInMemory();
  assert.equal(readChatSends(tabId).length, 1);
  assert.deepEqual(readChatSends(tabId)[0].draft?.attachments, ["/opaque/file"]);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "failed");
});

test("JSONL matched-row identity survives the persisted delivery ledger", () => {
  const tabId = "matched-row-restart";
  const matchedRow = {
    source: { source_session_id: "source-session", record: {
      kind: "jsonl" as const, incarnation: "incarnation", record_start: 40, record_row: 2,
    } },
    ordinal: 2,
  };
  addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1700000000000 });
  observeDeliveryStatus({ tab_id: tabId, send_id: "tx", correlationId: "token", state: "delivered",
    error: null, ledger_id: "ledger", sequence: 1, retained_from: 1, matched_row: matchedRow });

  forgetChatSendsInMemory();

  assert.deepEqual(readChatSends(tabId)[0]?.matchedRow, matchedRow);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "delivered");
});

test("retry clears the prior row claim before creating a new send identity", () => {
  const tabId = "matched-row-retry";
  addChatSend(tabId, { id: "bubble", correlationId: "old-token", content: "opaque", sentAtMs: 1700000000000 });
  const matchedRow = { source: { source_session_id: "source", record: {
    kind: "structured" as const, thread_id: "source", item_id: "old-item",
  } }, ordinal: 1 };
  observeDeliveryStatus({ tab_id: tabId, send_id: "old-tx", correlationId: "old-token", state: "failed",
    error: null, ledger_id: "ledger", sequence: 1, retained_from: 1, matched_row: matchedRow });

  const retry = beginChatSendRetry(tabId, "bubble");

  assert(retry);
  assert.equal(retry.matchedRow, undefined);
  assert.notEqual(retry.correlationId, "old-token");
});

test("a lost ledger failure cannot leave a stale Retry after reconnect", () => {
  const tabId = "lost-ledger";
  addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1700000000000 });
  observeDeliveryStatus({ tab_id: tabId, send_id: "tx", correlationId: "token", state: "failed",
    error: null, ledger_id: "old-ledger", sequence: 3, retained_from: 1 });
  reconcileDeliveryStatuses(tabId, []);
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
  assert.equal(readChatSends(tabId).length, 1);
});

test("Retry mints a fresh correlation id on an insecure origin without crypto.randomUUID", () => {
  const tabId = "retry-insecure";
  const original = Object.getOwnPropertyDescriptor(globalThis, "crypto");
  Object.defineProperty(globalThis, "crypto", { configurable: true, value: { getRandomValues: (b: Uint8Array) => b.fill(7) } });
  try {
    addChatSend(tabId, { id: "bubble", correlationId: "original-token", content: "opaque", sentAtMs: 1700000000000 });
    observeDeliveryStatus({ tab_id: tabId, send_id: "first", state: "failed", ledger_id: "ledger",
      sequence: 1, retained_from: 1, error: null, correlationId: "original-token" });
    const retry = beginChatSendRetry(tabId, "bubble");
    assert(retry);
    assert.match(retry.correlationId ?? "", /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  } finally {
    if (original) Object.defineProperty(globalThis, "crypto", original);
  }
});
