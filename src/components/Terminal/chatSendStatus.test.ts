import assert from "node:assert/strict";
import { beforeEach, test } from "vitest";

import { applyDeliveryStatus, deliveryState, type DeliverySend } from "./chatDeliveryModel";
import { __setInvokeForTest } from "../../lib/bridge";
import { isProvenAdmissionRefusal, reconcileTrackedChatMessages, sendTrackedChatMessage, type ChatSendStatus } from "./chatSendStatus";
import { addChatSend, beginChatSendRetry, forgetChatSendsInMemory, observeDeliveryStatus, readChatSends,
  __setChatSendRepositoryForTest, ensureChatSendTabHydrated, chatSendTabHydrationState } from "./chatSendStore";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";

beforeEach(() => __setChatSendRepositoryForTest(new MemoryChatSendRepository()));

test("chat backend not-applicable admission settles across reload without Retry or resubmission", async () => {
  const tabId = "backend-admission";
  addChatSend(tabId, { id: "bubble", correlationId: "backend-token", content: "opaque", sentAtMs: 1 });
  let admissions = 0;
  __setInvokeForTest(async <T>(command: string) => {
    if (command === "chat_send_admissions") {
      admissions++;
      return [{ correlationId: "backend-token", evidence: "not_applicable" }] as T;
    }
    assert.ok(command === "chat_send_status" || command === "chat_send_dismissals");
    return [] as T;
  });
  await reconcileTrackedChatMessages(tabId);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "uncertain");
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
  forgetChatSendsInMemory();
  await ensureChatSendTabHydrated(tabId);
  await reconcileTrackedChatMessages(tabId);
  assert.equal(admissions, 1);
  assert.equal(readChatSends(tabId)[0].admissionSettled, true);
});

function heldStatus(): ChatSendStatus {
  return {
    send_id: "send-1",
    tab_id: "tab-1",
    state: "held",
    error: null,
    ledger_id: "ledger-1",
    sequence: 1,
    retained_from: 1,
    correlationId: "message-1",
  };
}

test("restart reconciliation wires durable states onto each persisted held bubble without resubmitting", async () => {
  for (const state of ["held", "failed", "delivered", "submit_unconfirmed"] as const) {
    const tabId = `restart-${state}`;
    const prior = { ...heldStatus(), tab_id: tabId };
    addChatSend(tabId, { id: "bubble", correlationId: "message-1", content: "payload", sentAtMs: 1 });
    observeDeliveryStatus(prior);
    forgetChatSendsInMemory();
    await ensureChatSendTabHydrated(tabId);
    const restored = { ...prior, ledger_id: "new-daemon", state };
    const calls: unknown[] = [];
    __setInvokeForTest(async <T>(command: string, args?: unknown) => {
      calls.push({ command, args });
      return [restored] as T;
    });
    await reconcileTrackedChatMessages(tabId);
    forgetChatSendsInMemory();
    await ensureChatSendTabHydrated(tabId);
    const sends = readChatSends(tabId);
    assert.equal(sends.length, 1);
    assert.equal(sends[0].id, "bubble");
    assert.equal(sends[0].status?.state, state);
    assert.equal(sends[0].status?.ledger_id, "new-daemon");
    assert.deepEqual(calls, [{ command: "chat_send_status", args: { tabId } }, { command: "chat_send_dismissals", args: { tabId } }]);
    assert.equal(beginChatSendRetry(tabId, "bubble") !== undefined, state === "failed");
  }
});

test("an absent receipt preserves held input as queued without inventing Retry", async () => {
  const tabId = "missing-restart-receipt";
  addChatSend(tabId, { id: "bubble", correlationId: "message-1", content: "payload", sentAtMs: 1 });
  observeDeliveryStatus({ ...heldStatus(), tab_id: tabId });
  forgetChatSendsInMemory();
  await ensureChatSendTabHydrated(tabId);
  __setInvokeForTest(async <T>() => [] as T);
  await reconcileTrackedChatMessages(tabId);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "queued");
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
});

test("status reconciliation waits for full hydration and skips failed storage without delivery inference", async () => {
  const tabId = "reconcile-hydration-boundary";
  let finishHydration!: () => void;
  const repository = new MemoryChatSendRepository();
  repository.hydrationGate = new Promise<void>((resolve) => { finishHydration = resolve; });
  __setChatSendRepositoryForTest(repository);
  const commands: string[] = [];
  __setInvokeForTest(async <T,>(command: string) => {
    commands.push(command);
    return [] as T;
  });

  const pendingReconciliation = reconcileTrackedChatMessages(tabId);
  const duplicateReconciliation = reconcileTrackedChatMessages(tabId);
  assert.equal(pendingReconciliation, duplicateReconciliation, "overlapping pending reads share the hydration boundary");
  await Promise.resolve();
  assert.equal(chatSendTabHydrationState(tabId), "pending");
  assert.deepEqual(commands, [], "pending storage cannot trigger empty-ledger reconciliation");
  finishHydration();
  await pendingReconciliation;
  assert.deepEqual(commands, ["chat_send_status", "chat_send_dismissals"], "full hydration enables authoritative reconciliation");

  const failedRepository = new MemoryChatSendRepository();
  failedRepository.failHydration = true;
  __setChatSendRepositoryForTest(failedRepository);
  commands.length = 0;
  addChatSend(tabId, { id: "live", correlationId: "live", content: "still sending", sentAtMs: 1 });
  await reconcileTrackedChatMessages(tabId);
  assert.equal(chatSendTabHydrationState(tabId), "error");
  assert.deepEqual(commands, [], "storage failure cannot run deletion or delivery reconciliation");
  assert.equal(deliveryState(readChatSends(tabId)[0]!), "sending", "storage failure leaves live send status unchanged");
});

test("held send enters the backend ledger once", async () => {
  const calls: Array<{ command: string; args: unknown }> = [];
  const accepted = heldStatus();
  __setInvokeForTest(async <T>(command: string, args?: unknown) => {
    calls.push({ command, args });
    return accepted as T;
  });

  const result = await sendTrackedChatMessage("tab-1", "message-1", "hello");

  assert.deepEqual(result, accepted);
  assert.deepEqual(calls, [{
    command: "chat_send_text",
    args: { tabId: "tab-1", text: "hello", clientToken: "message-1" },
  }]);
});

test("request rejection is logged and rethrown without a UI status channel", async () => {
  const failure = new Error("transport unavailable");
  __setInvokeForTest(async () => {
    throw failure;
  });
  const previousWarn = console.warn;
  const warnings: unknown[][] = [];
  console.warn = (...args: unknown[]) => warnings.push(args);
  try {
    await assert.rejects(
      sendTrackedChatMessage("tab-1", "message-2", "hello"),
      failure,
    );
  } finally {
    console.warn = previousWarn;
  }

  assert.equal(warnings.length, 1);
  assert.match(String(warnings[0]?.[0]), /\[chat-send\]/);
});

test("only proven pre-admission refusal allows Retry", () => {
  assert.equal(isProvenAdmissionRefusal(new Error("user send capacity: 32 unsettled admissions")), true);
  assert.equal(isProvenAdmissionRefusal(new Error("startup override requires the provider launch command to be written")), true);
  assert.equal(isProvenAdmissionRefusal(new Error("Startup override requires authenticated owner authority")), true);
  assert.equal(isProvenAdmissionRefusal(new Error("admission_unknown: startup override response lost")), false);
  assert.equal(isProvenAdmissionRefusal(new Error("admission_unknown: durability could not be qualified")), false);
  assert.equal(isProvenAdmissionRefusal(new TypeError("Failed to fetch")), false);
  assert.equal(isProvenAdmissionRefusal(new Error("connection reset")), false);
});

test("reconnect resolves a response-less refused admission using durable absence proof", async () => {
  const tabId = "offline-admission";
  addChatSend(tabId, { id: "offline-bubble", correlationId: "offline-token", content: "opaque", sentAtMs: 1 });
  const calls: string[] = [];
  __setInvokeForTest(async <T>(command: string) => {
    calls.push(command);
    if (command === "chat_send_text") throw new TypeError("opaque-network-failure");
    if (command === "chat_send_status") return [] as T;
    if (command === "chat_send_admissions") return [{ correlationId: "offline-token", evidence: "absent" }] as T;
    throw new Error(command);
  });
  await assert.rejects(sendTrackedChatMessage(tabId, "offline-token", "opaque"));
  await reconcileTrackedChatMessages(tabId);
  forgetChatSendsInMemory();
  await ensureChatSendTabHydrated(tabId);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "failed");
  assert.equal(calls.filter((command) => command === "chat_send_text").length, 1);
  assert.notEqual(beginChatSendRetry(tabId, "offline-bubble"), undefined);
});

test("a send request that hit its deadline resolves to Retry once durable absence is proven", async () => {
  const tabId = "deadline-admission";
  addChatSend(tabId, { id: "deadline-bubble", correlationId: "deadline-token", content: "opaque", sentAtMs: 1 });
  __setInvokeForTest(async <T>(command: string) => {
    if (command === "chat_send_text") throw new DOMException("opaque", "TimeoutError");
    if (command === "chat_send_status") return [] as T;
    if (command === "chat_send_admissions") return [{ correlationId: "deadline-token", evidence: "absent" }] as T;
    throw new Error(command);
  });
  await assert.rejects(sendTrackedChatMessage(tabId, "deadline-token", "opaque"));
  await reconcileTrackedChatMessages(tabId);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "failed");
  assert.notEqual(beginChatSendRetry(tabId, "deadline-bubble"), undefined);
});

test("reconnect leaves an ambiguous response uncertain even when no admission is retained", async () => {
  const tabId = "ambiguous-admission";
  addChatSend(tabId, { id: "ambiguous-bubble", correlationId: "ambiguous-token", content: "opaque", sentAtMs: 1 });
  __setInvokeForTest(async <T>(command: string) => {
    if (command === "chat_send_text") throw new Error("opaque-ambiguous-response");
    if (command === "chat_send_status") return [] as T;
    return [{ correlationId: "ambiguous-token", evidence: "absent" }] as T;
  });
  await assert.rejects(sendTrackedChatMessage(tabId, "ambiguous-token", "opaque"));
  await reconcileTrackedChatMessages(tabId);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "uncertain");
  assert.equal(beginChatSendRetry(tabId, "ambiguous-bubble"), undefined);
});


test("model command hold shows queued, then sending and delivered on the same bubble", async () => {
  let submissions = 0;
  const held = { ...heldStatus(), pending_reason: "awaiting_model_command" };
  __setInvokeForTest(async <T>() => {
    submissions += 1;
    return held as T;
  });
  const receipt = await sendTrackedChatMessage("tab-1", "message-1", "opaque payload");
  const original: DeliverySend = {
    id: "bubble", correlationId: "message-1", content: "opaque payload", sentAtMs: 1000,
  };
  const queued = applyDeliveryStatus(original, receipt);
  assert.equal(deliveryState(queued), "queued");
  const deadline = applyDeliveryStatus(queued, { ...receipt, pending_reason: "model_command_completion_unconfirmed" });
  assert.equal(deliveryState(deadline), "queued");
  const written = applyDeliveryStatus(deadline, { ...receipt, state: "written", pending_reason: undefined });
  assert.equal(deliveryState(written), "sending");
  const delivered = applyDeliveryStatus(written, { ...receipt, state: "delivered", pending_reason: undefined });
  assert.equal(deliveryState(delivered), "delivered");
  assert.equal(delivered.id, original.id);
  assert.equal(submissions, 1);
});

test("Delete persists a dismissal with one RPC and never submits or retries its payload", async () => {
  const { deleteTrackedChatMessage } = await import("./chatSendStatus");
  const repository = new MemoryChatSendRepository();
  __setChatSendRepositoryForTest(repository);
  const tabId = "delete-rpc";
  const original = { id: "delete-bubble", correlationId: "delete-token", content: "opaque", sentAtMs: 1700000000000 };
  addChatSend(tabId, original);
  observeDeliveryStatus({ ...heldStatus(), tab_id: tabId, correlationId: original.correlationId, state: "failed" });
  const calls: string[] = [];
  let release!: (receipt: unknown) => void;
  __setInvokeForTest(async <T>(command: string) => {
    calls.push(command);
    return await new Promise<unknown>((resolve) => { release = resolve; }) as T;
  });
  const deleting = deleteTrackedChatMessage(tabId, original.id);
  assert.equal(beginChatSendRetry(tabId, original.id), undefined);
  await deleteTrackedChatMessage(tabId, original.id);
  assert.equal(readChatSends(tabId)[0].dismissed, undefined);
  release({ send_id: "send-1", send: original });
  await deleting;
  forgetChatSendsInMemory();
  await ensureChatSendTabHydrated(tabId);
  assert.deepEqual(readChatSends(tabId), []);
  const marker = await repository.readDismissal(tabId, original.id);
  assert(marker);
  assert.equal(marker.sendId, original.id);
  assert.equal(marker.correlationId, original.correlationId);
  assert.equal("content" in marker, false);
  assert.deepEqual(calls, ["chat_send_delete"]);
  assert.equal(beginChatSendRetry(tabId, original.id), undefined);
  __setInvokeForTest(null);
});

test("Delete rejects a delivered bubble before making an RPC", async () => {
  const { deleteTrackedChatMessage } = await import("./chatSendStatus");
  const tabId = "delete-delivered";
  addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1 });
  observeDeliveryStatus({ ...heldStatus(), tab_id: tabId, correlationId: "token", state: "delivered" });
  let invoked = false;
  __setInvokeForTest(async <T>() => { invoked = true; return undefined as T; });
  await deleteTrackedChatMessage(tabId, "bubble");
  assert.equal(invoked, false);
  __setInvokeForTest(null);
});

test("an uncertain admission can be dismissed by its client token without replay", async () => {
  const { deleteTrackedChatMessage } = await import("./chatSendStatus");
  const tabId = "delete-no-receipt";
  const original = { id: "no-receipt", correlationId: "pending-token", content: "opaque", sentAtMs: 1, requestUncertain: true };
  addChatSend(tabId, original);
  let request: Record<string, unknown> | undefined;
  __setInvokeForTest(async <T>(command: string, args?: Record<string, unknown>) => {
    assert.equal(command, "chat_send_delete");
    request = args;
    return { send_id: original.id, send: original } as T;
  });
  await deleteTrackedChatMessage(tabId, original.id);
  assert.equal((request?.dismissal as { send_id: string }).send_id, original.id);
  assert.equal(readChatSends(tabId)[0].dismissed, true);
  __setInvokeForTest(null);
});
