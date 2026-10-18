import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act, createElement, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  addChatSend, beginChatSendRetry, failChatSendRequest, forgetChatSendsInMemory, removeChatSend,
  observeChatSendDismissal, observeDeliveryStatus, readChatSends, reconcileChatSendAdmission,
  reconcileDeliveryStatuses, recordChatSendAdmissionFailure, setChatSendDraft, useChatSends,
  __setChatSendRepositoryForTest, ensureChatSendTabHydrated, chatSendTabHydrationState, isChatSendTabHydrated,
  useChatSendSnapshot,
} from "./chatSendStore";
import { deliveryState } from "./chatDeliveryModel";
import type { ChatSendStatus } from "./chatSendStatus";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";
import { encodeChatSend, encodeChatSendTombstone } from "./chatSendDurableModel";
import type { ChatSendRepositoryPort } from "./chatSendRepository";

test("cold hydration is explicitly pending while local sends remain synchronous", async () => {
  const tabId = "cold-hydration";
  const persisted = encodeChatSend(tabId, { id: "restored", content: "stored", sentAtMs: 1 });
  let completeHydration!: (result: Awaited<ReturnType<ChatSendRepositoryPort["hydrateTab"]>>) => void;
  const repository: ChatSendRepositoryPort = {
    hydrateTab: () => new Promise((resolve) => { completeHydration = resolve; }),
    persist: async () => {},
    remove: async () => {},
    read: async () => undefined,
    subscribe: () => () => {},
    state: () => "pending",
    close: () => {},
  };
  __setChatSendRepositoryForTest(repository);
  try {
    assert.deepEqual(readChatSends(tabId), []);
    assert.equal(chatSendTabHydrationState(tabId), "pending");
    assert.equal(isChatSendTabHydrated(tabId), false);
    addChatSend(tabId, { id: "local", content: "immediate", sentAtMs: 2 });
    assert.deepEqual(readChatSends(tabId).map((send) => send.id), ["local"]);

    completeHydration({ records: [persisted], state: "ready" });
    await ensureChatSendTabHydrated(tabId);

    assert.deepEqual(readChatSends(tabId).map((send) => send.id).sort(), ["local", "restored"]);
    assert.equal(chatSendTabHydrationState(tabId), "ready");
    assert.equal(isChatSendTabHydrated(tabId), true);
  } finally {
    __setChatSendRepositoryForTest(null);
  }
});

test("snapshot hydration state is stable, changes after load, and local edits survive a late read", async () => {
  const tabId = "snapshot-late-hydration";
  const persisted = encodeChatSend(tabId, { id: "shared", content: "stored", sentAtMs: 1 });
  let completeHydration!: (result: Awaited<ReturnType<ChatSendRepositoryPort["hydrateTab"]>>) => void;
  let hydrateCalls = 0;
  const repository: ChatSendRepositoryPort = {
    hydrateTab: () => {
      hydrateCalls += 1;
      return new Promise((resolve) => { completeHydration = resolve; });
    },
    persist: async () => {},
    remove: async () => {},
    read: async () => undefined,
    subscribe: () => () => {},
    state: () => "pending",
    close: () => {},
  };
  __setChatSendRepositoryForTest(repository);
  const host = document.createElement("div");
  const root = createRoot(host);
  let snapshot!: ReturnType<typeof useChatSendSnapshot>;
  let rerender!: () => void;
  function Probe() {
    const [, setVersion] = useState(0);
    rerender = () => setVersion((version) => version + 1);
    snapshot = useChatSendSnapshot(tabId);
    return null;
  }

  try {
    await act(async () => root.render(createElement(Probe)));
    assert.equal(snapshot.hydration, "pending");
    assert.equal(snapshot.sends.length, 0);
    assert.equal(hydrateCalls, 1);

    await act(async () => {
      addChatSend(tabId, { id: "shared", content: "live edit", sentAtMs: 2 });
      addChatSend(tabId, { id: "new-live", content: "new session send", sentAtMs: 3 });
    });
    assert.deepEqual(snapshot.sends.map((send) => send.id), ["shared", "new-live"]);
    assert.equal(snapshot.sends[0]?.content, "live edit");
    assert.equal(snapshot.hydration, "pending");

    await act(async () => {
      completeHydration({ records: [persisted], state: "ready" });
      await ensureChatSendTabHydrated(tabId);
    });
    assert.equal(snapshot.hydration, "ready");
    assert.deepEqual(snapshot.sends.map((send) => send.id), ["shared", "new-live"]);
    assert.equal(snapshot.sends[0]?.content, "live edit");
    const readySnapshot = snapshot;
    await act(async () => rerender());
    assert.equal(snapshot, readySnapshot);
  } finally {
    await act(async () => root.unmount());
    __setChatSendRepositoryForTest(null);
  }
});

test("a persisted tombstone cannot erase a send created in the current session during hydration", async () => {
  const tabId = "live-over-tombstone";
  let completeHydration!: () => void;
  const repository = new MemoryChatSendRepository();
  repository.hydrationGate = new Promise<void>((resolve) => { completeHydration = resolve; });
  repository.records.set(JSON.stringify([tabId, "live"]), encodeChatSendTombstone(tabId, "live"));
  __setChatSendRepositoryForTest(repository);
  try {
    addChatSend(tabId, { id: "live", correlationId: "live-token", content: "current session", sentAtMs: 1 });
    observeDeliveryStatus({ tab_id: tabId, send_id: "live-tx", correlationId: "live-token", state: "written",
      error: null, ledger_id: "ledger", sequence: 1, retained_from: 1 });
    completeHydration();
    await ensureChatSendTabHydrated(tabId);

    const live = readChatSends(tabId).find((send) => send.id === "live");
    assert.equal(live?.content, "current session");
    assert.equal(live?.status?.state, "written");
  } finally {
    __setChatSendRepositoryForTest(null);
  }
});

test("hydration failure is visible without disabling local sends or inventing delivery failure", async () => {
  const tabId = "snapshot-hydration-failure";
  const repository = new MemoryChatSendRepository();
  repository.failHydration = true;
  __setChatSendRepositoryForTest(repository);
  const host = document.createElement("div");
  const root = createRoot(host);
  let snapshot!: ReturnType<typeof useChatSendSnapshot>;
  function Probe() {
    snapshot = useChatSendSnapshot(tabId);
    return null;
  }

  try {
    await act(async () => root.render(createElement(Probe)));
    await act(async () => ensureChatSendTabHydrated(tabId));
    assert.equal(snapshot.hydration, "error");
    await act(async () => addChatSend(tabId, { id: "live", content: "still usable", sentAtMs: 1 }));
    assert.equal(snapshot.hydration, "error");
    assert.equal(snapshot.sends[0]?.id, "live");
    assert.equal(deliveryState(snapshot.sends[0]!), "sending");
  } finally {
    await act(async () => root.unmount());
    __setChatSendRepositoryForTest(null);
  }
});

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
  run: (probe: { writes: Array<{ tabId: string; sendId: string; bytes: number }>; renderCount: () => number }) => Promise<void>,
): Promise<void> {
  const repository = new MemoryChatSendRepository();
  __setChatSendRepositoryForTest(repository);

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
    await run({ writes: repository.writes, renderCount: () => renders });
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setChatSendRepositoryForTest(null);
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
    await act(async () => ensureChatSendTabHydrated(tabId));
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
    await act(async () => ensureChatSendTabHydrated(tabId));
    const restored = readChatSends(tabId).find((send) => send.id === admission.id);
    assert.equal(deliveryState(restored!), "failed");
  });
});

test("synthetic multi-tab records write only one bounded changed record", async (t) => {
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
  const serializedLedger = JSON.stringify(rawSends);
  const collectionBytes = Buffer.byteLength(serializedLedger, "utf8");
  forgetChatSendsInMemory();

  const tabId = "synthetic-tab-0";
  const receipt = {
    send_id: "synthetic-receipt",
    send: { id: "synthetic-send-0", correlationId: "synthetic-token-0", content: "opaque", sentAtMs: 100 },
  };
  try {
    await withStoreProbe(tabId, async ({ writes, renderCount }) => {
      const before = renderCount();
      await act(async () => observeChatSendDismissal(tabId, receipt));
      assert.equal(writes.length, 1);
      assert.equal(renderCount(), before + 1);
      assert(writes[0].bytes < collectionBytes);

      for (let index = 0; index < 4; index += 1) {
        await act(async () => observeChatSendDismissal(tabId, receipt));
      }
      assert.equal(writes.length, 1);
      assert.equal(renderCount(), before + 1);
      t.diagnostic(`synthetic multi-tab ledger: 32 tabs, 1,024 records; collection UTF-8 bytes=${collectionBytes}; changed record UTF-8 bytes=${writes[0].bytes}; writes=1`);
    });
  } finally {
    forgetChatSendsInMemory();
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

test("unprepared attachment sends survive restart on their original bubble", async () => {
  const tabId = "restored-preparation";
  await withStoreProbe(tabId, async () => {
    await act(async () => {
      addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1700000000000 });
      setChatSendDraft(tabId, "bubble", { text: "opaque", attachments: ["/opaque/file"], recordedVoiceAttachments: [], capsules: {}, reply: null });
      failChatSendRequest(tabId, "bubble");
      forgetChatSendsInMemory();
    });
    await act(async () => ensureChatSendTabHydrated(tabId));
    assert.equal(readChatSends(tabId).length, 1);
    assert.deepEqual(readChatSends(tabId)[0].draft?.attachments, ["/opaque/file"]);
    assert.equal(deliveryState(readChatSends(tabId)[0]), "failed");
  });
});

test("JSONL matched-row identity survives the persisted delivery ledger", async () => {
  const tabId = "matched-row-restart";
  const matchedRow = {
    source: { source_session_id: "source-session", record: {
      kind: "jsonl" as const, incarnation: "incarnation", record_start: 40, record_row: 2,
    } },
    ordinal: 2,
  };
  await withStoreProbe(tabId, async () => {
    await act(async () => {
      addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1700000000000 });
      observeDeliveryStatus({ tab_id: tabId, send_id: "tx", correlationId: "token", state: "delivered",
        error: null, ledger_id: "ledger", sequence: 1, retained_from: 1, matched_row: matchedRow });
      forgetChatSendsInMemory();
    });
    await act(async () => ensureChatSendTabHydrated(tabId));

    assert.deepEqual(readChatSends(tabId)[0]?.matchedRow, matchedRow);
    assert.equal(deliveryState(readChatSends(tabId)[0]), "delivered");
  });
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
