import { useCallback, useEffect, useSyncExternalStore } from "react";
import type { ChatSend, SendEcho } from "@codeterm/chat-core";
import { applyDeliveryStatus, canDeleteChatSend, deliveryState, reconcileDeliveryStatusesModel, reconcileAdmissionEvidence, type AdmissionEvidence, type DeliveryDraft, type DeliverySend } from "./chatDeliveryModel";
import type { ChatSendStatus } from "./chatSendStatus";
import { uid } from "../../lib/uid";
import {
  createChatSendRepository,
  type ChatSendRepositoryPort,
} from "./chatSendRepository";
import {
  decodeChatSend,
  mergeChatSendRecord,
  sameDurableSend,
  type ChatSendDurableRecord,
} from "./chatSendDurableModel";

const MAX_SENDS_PER_TAB = 64;
const MAX_TABS = 32;
const EMPTY: readonly DeliverySend[] = [];
export type ChatSendHydrationState = "pending" | "ready" | "error";
export interface ChatSendSnapshot {
  sends: readonly DeliverySend[];
  hydration: ChatSendHydrationState;
}

/** The ledger stays in memory for synchronous React reads; IndexedDB is record persistence only. */
const sendsByTab = new Map<string, readonly DeliverySend[]>();
const listeners = new Map<string, Set<() => void>>();
const attemptedHydrationTabs = new Set<string>();
const hydrationStates = new Map<string, ChatSendHydrationState>();
const hydrationTasks = new Map<string, Promise<void>>();
const hydrationRemovedIds = new Map<string, Set<string>>();
const snapshotsByTab = new Map<string, ChatSendSnapshot>();
const deleting = new Set<string>();
const deletionKey = (tabId: string, id: string) => JSON.stringify([tabId, id]);
const repository = createChatSendRepository();
let activeRepository: ChatSendRepositoryPort = repository;
let unsubscribeRepository = activeRepository.subscribe(onRepositoryChange);

function notify(tabId: string): void {
  for (const listener of listeners.get(tabId) ?? []) listener();
}

function notifyIfLoadedSnapshotChanged(tabId: string): void {
  const snapshot = snapshotsByTab.get(tabId);
  if (snapshot && snapshot.sends !== (sendsByTab.get(tabId) ?? EMPTY)) notify(tabId);
}

function setTab(tabId: string, next: readonly DeliverySend[]): void {
  if (next.length === 0) sendsByTab.delete(tabId);
  else {
    sendsByTab.delete(tabId);
    sendsByTab.set(tabId, next);
  }
}

function ensureTabHydrated(tabId: string): Promise<void> {
  if (attemptedHydrationTabs.has(tabId)) return Promise.resolve();
  const existingTask = hydrationTasks.get(tabId);
  if (existingTask) return existingTask;
  const baseline = new Map((sendsByTab.get(tabId) ?? EMPTY).map((send) => [send.id, send]));
  hydrationStates.set(tabId, "pending");
  hydrationRemovedIds.set(tabId, new Set());
  notifyIfLoadedSnapshotChanged(tabId);
  const task = activeRepository.hydrateTab(tabId).then(({ records, state }) => {
    let current = [...(sendsByTab.get(tabId) ?? EMPTY)];
    const byId = new Map(current.map((send) => [send.id, send]));
    const removedIds = hydrationRemovedIds.get(tabId);
    const rows = new Map(records.map((record) => [record.sendId, record]));
    const persistedOrder: string[] = [];
    for (const [sendId, record] of rows) {
      const before = baseline.get(sendId);
      const after = byId.get(sendId);
      if (record.deleted) {
        if (!after || after === before) byId.delete(sendId);
        continue;
      }
      if (!after && removedIds?.has(sendId)) continue;
      const persisted = decodeChatSend(record);
      if (!persisted) continue;
      persistedOrder.push(sendId);
      const merged = after ? mergeChatSendRecord(persisted, before, after) : persisted;
      byId.set(sendId, merged);
    }
    const orderedIds = new Set(persistedOrder);
    const next = [
      ...persistedOrder.flatMap((sendId) => {
        const send = byId.get(sendId);
        return send ? [send] : [];
      }),
      ...[...byId.values()].filter((send) => !orderedIds.has(send.id)),
    ];
    if (!sameList(current, next)) {
      setTab(tabId, next);
    }
    hydrationStates.set(tabId, state === "ready" ? "ready" : "error");
    notify(tabId);
  }).catch(() => {
    hydrationStates.set(tabId, "error");
    notify(tabId);
  }).finally(() => {
    attemptedHydrationTabs.add(tabId);
    hydrationTasks.delete(tabId);
    hydrationRemovedIds.delete(tabId);
  });
  hydrationTasks.set(tabId, task);
  return task;
}

function sameList(left: readonly DeliverySend[], right: readonly DeliverySend[]): boolean {
  return left.length === right.length && left.every((send, index) => send === right[index]);
}

function persistDiff(tabId: string, previous: readonly DeliverySend[], next: readonly DeliverySend[]): void {
  const before = new Map(previous.map((send) => [send.id, send]));
  const after = new Map(next.map((send) => [send.id, send]));
  for (const [sendId, current] of after) {
    const prior = before.get(sendId);
    if (prior === current || (prior && sameDurableSend(prior, current))) continue;
    void activeRepository.persist(tabId, prior, current);
  }
  for (const [sendId, prior] of before) {
    if (!after.has(sendId)) void activeRepository.remove(tabId, sendId, prior);
  }
}

function applyPersistedRecord(tabId: string, sendId: string, record?: ChatSendDurableRecord): void {
  const current = sendsByTab.get(tabId) ?? EMPTY;
  const existing = current.find((send) => send.id === sendId);
  if (record?.deleted) {
    if (!existing) return;
    setTab(tabId, current.filter((send) => send.id !== sendId));
    notify(tabId);
    return;
  }
  const persisted = record ? decodeChatSend(record) : null;
  if (!persisted) return;
  const merged = existing ? mergeChatSendRecord(existing, undefined, persisted) : persisted;
  if (existing && sameDurableSend(existing, merged)) return;
  setTab(tabId, existing ? current.map((send) => send.id === sendId ? merged : send) : [...current, merged]);
  notify(tabId);
}

function onRepositoryChange(tabId: string, sendId: string): void {
  void activeRepository.read(tabId, sendId).then((record) => applyPersistedRecord(tabId, sendId, record));
}

export function readChatSends(tabId: string): readonly DeliverySend[] {
  void ensureTabHydrated(tabId);
  return sendsByTab.get(tabId) ?? EMPTY;
}

function readChatSendSnapshot(tabId: string): ChatSendSnapshot {
  const sends = sendsByTab.get(tabId) ?? EMPTY;
  const hydration = hydrationStates.get(tabId) ?? "pending";
  const current = snapshotsByTab.get(tabId);
  if (current?.sends === sends && current.hydration === hydration) return current;
  const next = { sends, hydration };
  snapshotsByTab.set(tabId, next);
  return next;
}

export function ensureChatSendTabHydrated(tabId: string): Promise<void> {
  return ensureTabHydrated(tabId);
}

export function isChatSendTabHydrated(tabId: string): boolean {
  return hydrationStates.get(tabId) === "ready";
}

export function chatSendTabHydrationState(tabId: string): ChatSendHydrationState {
  return hydrationStates.get(tabId) ?? "pending";
}

export function chatSendPersistenceState(): ReturnType<ChatSendRepositoryPort["state"]> {
  return activeRepository.state();
}

export function __setChatSendRepositoryForTest(next: ChatSendRepositoryPort | null): void {
  unsubscribeRepository();
  activeRepository = next ?? repository;
  unsubscribeRepository = activeRepository.subscribe(onRepositoryChange);
  forgetChatSendsInMemory();
}

export function addChatSend(tabId: string, send: ChatSend): void {
  if (readChatSends(tabId).some((existing) => existing.id === send.id)) return;
  const current: DeliverySend[] = [...readChatSends(tabId), send];
  publish(tabId, [...current.filter((item) => item.dismissed), ...current.filter((item) => !item.dismissed).slice(-MAX_SENDS_PER_TAB)]);
}

export function removeChatSend(tabId: string, id: string): void {
  const current = readChatSends(tabId);
  const next = current.filter((send) => send.id !== id && send.correlationId !== id);
  if (next.length !== current.length) publish(tabId, next);
}

export function observeDeliveryStatus(status: ChatSendStatus): void {
  if (status.system_send && status.correlationId) {
    addChatSend(status.tab_id, {
      id: status.send_id, correlationId: status.correlationId,
      content: status.system_send.content, sentAtMs: status.system_send.sent_at_ms,
    });
  }
  const current = readChatSends(status.tab_id);
  const next = current.map((send) => applyDeliveryStatus(send, status));
  if (next.some((send, index) => send !== current[index])) publish(status.tab_id, next);
}

export function failChatSendRequest(tabId: string, id: string): void {
  const current = readChatSends(tabId);
  const next = current.map((send) => (send.id === id || send.correlationId === id) && deliveryState(send) !== "delivered"
    ? { ...send, requestFailed: true } : send);
  if (next.some((send, index) => send !== current[index])) publish(tabId, next);
}

export function reconcileChatSendAdmission(tabId: string, correlationId: string, evidence: AdmissionEvidence): void {
  const current = readChatSends(tabId);
  const next = current.map((send) => send.correlationId === correlationId
    ? reconcileAdmissionEvidence(send, evidence) : send);
  if (next.some((send, index) => send !== current[index])) publish(tabId, next);
}

export function recordChatSendAdmissionFailure(tabId: string, correlationId: string, hadResponse: boolean): void {
  const current = readChatSends(tabId);
  const next = current.map((send) => {
    if (send.correlationId !== correlationId) return send;
    const withResponse = send.requestHadResponse === hadResponse
      ? send
      : { ...send, requestHadResponse: hadResponse };
    return reconcileAdmissionEvidence(withResponse, "unknown");
  });
  if (next.some((send, index) => send !== current[index])) publish(tabId, next);
}

export function reconcileDeliveryStatuses(
  tabId: string, statuses: readonly ChatSendStatus[], requested?: readonly DeliverySend[],
): void {
  for (const status of statuses) {
    if (status.system_send && status.correlationId) observeDeliveryStatus(status);
  }
  const current = readChatSends(tabId);
  const next = reconcileDeliveryStatusesModel(current, statuses, requested);
  if (next.some((send, index) => send !== current[index])) publish(tabId, next);
}

export function beginChatSendRetry(tabId: string, id: string): DeliverySend | undefined {
  const current = readChatSends(tabId);
  const send = current.find((item) => item.id === id);
  if (!send || deleting.has(deletionKey(tabId, id)) || send.dismissed || send.status?.system_send || deliveryState(send) !== "failed") return;
  const next = { ...send, requestFailed: false, requestUncertain: false, requestHadResponse: undefined, status: undefined,
    matchedRow: undefined, correlationId: uid() };
  publish(tabId, current.map((item) => item.id === id ? next : item));
  return next;
}

export function setChatSendDraft(tabId: string, id: string, draft: DeliveryDraft | undefined): void {
  const current = readChatSends(tabId);
  const next = current.map((send) => send.id === id && !sameDraft(send.draft, draft) ? { ...send, draft } : send);
  if (next.some((send, index) => send !== current[index])) publish(tabId, next);
}

function sameDraft(left: DeliveryDraft | undefined, right: DeliveryDraft | undefined): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

/** Content can morph (a transcription placeholder); the send's key never does. */
export function updateChatSendContent(tabId: string, id: string, content: string): void {
  const current = readChatSends(tabId);
  if (!current.some((send) => send.id === id && send.content !== content)) return;
  publish(tabId, current.map((send) => (send.id === id ? { ...send, content } : send)));
}

/** An echo identity is recorded once; later passes never re-pair a reconciled send. */
export function recordChatSendEchoes(tabId: string, echoes: ReadonlyMap<string, SendEcho>): void {
  const current = readChatSends(tabId);
  if (!current.some((send) => send.echo == null && echoes.has(send.id))) return;
  publish(tabId, current.map((send) => {
    const echo = echoes.get(send.id);
    return send.echo != null || echo == null || deliveryState(send) === "failed" ? send : { ...send, echo };
  }));
}

export function chatSendAt(tabId: string, id: string | undefined): number | null {
  if (!id) return null;
  return readChatSends(tabId).find((send) => send.id === id || send.correlationId === id)?.sentAtMs ?? null;
}

/** Drops the in-memory copy as a restart does; callers await the tab hydration boundary. */
export function forgetChatSendsInMemory(): void {
  sendsByTab.clear();
  attemptedHydrationTabs.clear();
  hydrationStates.clear();
  hydrationTasks.clear();
  hydrationRemovedIds.clear();
  snapshotsByTab.clear();
  for (const set of listeners.values()) for (const listener of set) listener();
}

function publish(tabId: string, next: readonly DeliverySend[]): void {
  const previous = sendsByTab.get(tabId) ?? EMPTY;
  if (hydrationStates.get(tabId) === "pending") {
    const retainedIds = new Set(next.map((send) => send.id));
    let removed = hydrationRemovedIds.get(tabId);
    if (!removed) hydrationRemovedIds.set(tabId, (removed = new Set()));
    for (const send of previous) if (!retainedIds.has(send.id)) removed.add(send.id);
  }
  setTab(tabId, next);
  while (sendsByTab.size > MAX_TABS) {
    const oldest = sendsByTab.keys().next().value;
    if (oldest == null || oldest === tabId) break;
    sendsByTab.delete(oldest);
  }
  persistDiff(tabId, previous, next);
  notify(tabId);
}

function subscribe(tabId: string, onChange: () => void): () => void {
  const existing = listeners.get(tabId) ?? new Set<() => void>();
  existing.add(onChange);
  listeners.set(tabId, existing);
  return () => {
    existing.delete(onChange);
    if (existing.size === 0) listeners.delete(tabId);
  };
}

export function useChatSends(tabId: string): readonly DeliverySend[] {
  return useChatSendSnapshot(tabId).sends;
}

export function useChatSendSnapshot(tabId: string): ChatSendSnapshot {
  const subscribeToTab = useCallback((onChange: () => void) => subscribe(tabId, onChange), [tabId]);
  const read = useCallback(() => readChatSendSnapshot(tabId), [tabId]);
  const snapshot = useSyncExternalStore(subscribeToTab, read, read);
  useEffect(() => { void ensureTabHydrated(tabId); }, [tabId]);
  return snapshot;
}

export type ChatSendDismissal = { send_id: string; send: ChatSend };

export function observeChatSendDismissal(tabId: string, receipt: ChatSendDismissal): void {
  const current = readChatSends(tabId);
  const matches = (send: DeliverySend) => send.correlationId === receipt.send.correlationId;
  let found = false;
  let needsDismissal = false;
  for (const send of current) {
    if (!matches(send)) continue;
    found = true;
    needsDismissal ||= send.dismissed !== true;
  }
  if (found && !needsDismissal) return;
  const next = found
    ? current.map((send) => matches(send) ? { ...send, dismissed: true } : send)
    : [...current, { ...receipt.send, dismissed: true }];
  publish(tabId, next);
}

export function beginChatSendDeletion(tabId: string, id: string): DeliverySend | undefined {
  const send = readChatSends(tabId).find((item) => item.id === id);
  const key = deletionKey(tabId, id);
  if (!send || !canDeleteChatSend(send) || deleting.has(key)) return;
  deleting.add(key);
  return send;
}

export function endChatSendDeletion(tabId: string, id: string): void {
  deleting.delete(deletionKey(tabId, id));
}

export function setDroppedUploads(tabId: string, id: string, names: string[]): void {
  const current = readChatSends(tabId);
  const next = current.map((send) => send.id === id && JSON.stringify(send.droppedUploads) !== JSON.stringify(names)
    ? { ...send, droppedUploads: names } : send);
  if (next.some((send, index) => send !== current[index])) publish(tabId, next);
}
