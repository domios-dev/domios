import { useCallback, useEffect, useSyncExternalStore } from "react";
import type { ChatSend, NormalizedChatMessage, SendEcho } from "@codeterm/chat-core";
import { applyDeliveryStatus, canRetryChatSend, canDeleteChatSend, deliveryState, reconcileDeliveryStatusesModel, reconcileAdmissionEvidence, type AdmissionEvidence, type DeliveryDraft, type DeliverySend } from "./chatDeliveryModel";
import type { ChatSendStatus } from "./chatSendStatus";
import { uid } from "../../lib/uid";
import {
  createChatSendRepository,
  type ChatSendRepositoryPort,
} from "./chatSendRepository";
import {
  chatSendRetention,
  chatSendDismissalIdentity,
  chatSendDismissalMatchesIdentity,
  decodeChatSend,
  encodeChatSendDismissalMarker,
  mergeChatSendRecord,
  mergeChatSendDismissalMarker,
  sameChatSendDismissalMarker,
  sameDurableSend,
  type ChatSendDismissalIdentity,
  type ChatSendDismissalMarker,
  type ChatSendDurableRecord,
} from "./chatSendDurableModel";

const MAX_SENDS_PER_TAB = 64;
const MAX_CACHED_TABS = 32;
const MAX_RECENT_DISMISSALS = 64;
const EMPTY: readonly DeliverySend[] = [];
const EMPTY_DISMISSALS: readonly ChatSendDismissalMarker[] = [];
export type ChatSendHydrationState = "pending" | "ready" | "error";
export interface ChatSendSnapshot {
  sends: readonly DeliverySend[];
  hydration: ChatSendHydrationState;
  dismissalRevision: number;
}

/** The ledger stays in memory for synchronous React reads; IndexedDB is record persistence only. */
const sendsByTab = new Map<string, readonly DeliverySend[]>();
const listeners = new Map<string, Set<() => void>>();
const cachedTabs = new Map<string, true>();
const hydrationStates = new Map<string, ChatSendHydrationState>();
const hydrationTasks = new Map<string, Promise<void>>();
const hydrationGenerations = new Map<string, number>();
const hydrationRemovedIds = new Map<string, Set<string>>();
const snapshotsByTab = new Map<string, ChatSendSnapshot>();
const dismissalMarkersByTab = new Map<string, Map<string, ChatSendDismissalMarker>>();
const dismissalLookupByTab = new Map<string, ChatSendDismissalLookup>();
const dismissalRevisionByTab = new Map<string, number>();
const deleting = new Set<string>();
const deletionKey = (tabId: string, id: string) => JSON.stringify([tabId, id]);
const repository = createChatSendRepository();
let activeRepository: ChatSendRepositoryPort = repository;
let unsubscribeRepository = activeRepository.subscribe(onRepositoryChange);

interface DismissalLookupResult {
  state: "pending" | "ready" | "error";
  markers: readonly ChatSendDismissalMarker[];
  task?: Promise<void>;
}

interface ChatSendDismissalLookup {
  rows: WeakMap<NormalizedChatMessage, DismissalLookupResult>;
  result: readonly ChatSendDismissalMarker[];
}

function notify(tabId: string): void {
  for (const listener of listeners.get(tabId) ?? []) listener();
}

function notifyIfLoadedSnapshotChanged(tabId: string): void {
  const snapshot = snapshotsByTab.get(tabId);
  if (snapshot && (snapshot.sends !== (sendsByTab.get(tabId) ?? EMPTY)
    || snapshot.hydration !== (hydrationStates.get(tabId) ?? "pending")
    || snapshot.dismissalRevision !== (dismissalRevisionByTab.get(tabId) ?? 0))) notify(tabId);
}

function setTab(tabId: string, next: readonly DeliverySend[]): void {
  touchTab(tabId);
  if (next.length === 0) sendsByTab.delete(tabId);
  else {
    sendsByTab.delete(tabId);
    sendsByTab.set(tabId, next);
  }
}

function touchTab(tabId: string): void {
  cachedTabs.delete(tabId);
  cachedTabs.set(tabId, true);
}

function rememberDismissalMarker(tabId: string, marker: ChatSendDismissalMarker): boolean {
  let markers = dismissalMarkersByTab.get(tabId);
  if (!markers) dismissalMarkersByTab.set(tabId, (markers = new Map()));
  const prior = markers.get(marker.sendId);
  const merged = mergeChatSendDismissalMarker(prior, marker);
  if (prior && sameChatSendDismissalMarker(prior, merged)) return false;
  markers.delete(marker.sendId);
  markers.set(marker.sendId, merged);
  while (markers.size > MAX_RECENT_DISMISSALS) {
    const oldest = markers.keys().next().value;
    if (oldest == null) break;
    markers.delete(oldest);
  }
  dismissalRevisionByTab.set(tabId, (dismissalRevisionByTab.get(tabId) ?? 0) + 1);
  return true;
}

function evictTab(tabId: string): void {
  cachedTabs.delete(tabId);
  sendsByTab.delete(tabId);
  hydrationStates.delete(tabId);
  hydrationRemovedIds.delete(tabId);
  snapshotsByTab.delete(tabId);
  dismissalMarkersByTab.delete(tabId);
  dismissalLookupByTab.delete(tabId);
  dismissalRevisionByTab.delete(tabId);
  hydrationGenerations.delete(tabId);
}

function trimCachedTabs(): void {
  const inactive = [...cachedTabs.keys()].filter((tabId) => !listeners.has(tabId) && !hydrationTasks.has(tabId));
  while (inactive.length > MAX_CACHED_TABS) {
    const tabId = inactive.shift();
    if (tabId) evictTab(tabId);
  }
}

function ensureTabHydrated(tabId: string): Promise<void> {
  touchTab(tabId);
  if (hydrationStates.get(tabId) === "ready" || hydrationStates.get(tabId) === "error") return Promise.resolve();
  const existingTask = hydrationTasks.get(tabId);
  if (existingTask) return existingTask;
  const generation = (hydrationGenerations.get(tabId) ?? 0) + 1;
  hydrationGenerations.set(tabId, generation);
  const baseline = new Map((sendsByTab.get(tabId) ?? EMPTY).map((send) => [send.id, send]));
  hydrationStates.set(tabId, "pending");
  hydrationRemovedIds.set(tabId, new Set());
  notifyIfLoadedSnapshotChanged(tabId);
  const task = activeRepository.hydrateTab(tabId).then(({ records, state }) => {
    if (hydrationGenerations.get(tabId) !== generation) return;
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
      setTab(tabId, retainTabSends(next));
    }
    hydrationStates.set(tabId, state === "ready" ? "ready" : "error");
    notify(tabId);
  }).catch(() => {
    if (hydrationGenerations.get(tabId) !== generation) return;
    hydrationStates.set(tabId, "error");
    notify(tabId);
  }).finally(() => {
    if (hydrationTasks.get(tabId) === task) hydrationTasks.delete(tabId);
    if (hydrationGenerations.get(tabId) === generation) hydrationRemovedIds.delete(tabId);
    trimCachedTabs();
  });
  hydrationTasks.set(tabId, task);
  trimCachedTabs();
  return task;
}

function retryTabHydration(tabId: string): Promise<void> {
  if (hydrationStates.get(tabId) !== "error") return ensureTabHydrated(tabId);
  hydrationStates.delete(tabId);
  return ensureTabHydrated(tabId);
}

function sameList(left: readonly DeliverySend[], right: readonly DeliverySend[]): boolean {
  return left.length === right.length && left.every((send, index) => send === right[index]);
}

function persistDiff(
  tabId: string, previous: readonly DeliverySend[], next: readonly DeliverySend[], logicalNext = next,
): void {
  const before = new Map(previous.map((send) => [send.id, send]));
  const after = new Map(next.map((send) => [send.id, send]));
  const logical = new Map(logicalNext.map((send) => [send.id, send]));
  for (const [sendId, current] of after) {
    const prior = before.get(sendId);
    if (prior === current || (prior && sameDurableSend(prior, current))) continue;
    void activeRepository.persist(tabId, prior, current);
  }
  for (const [sendId, prior] of before) {
    if (!after.has(sendId)) {
      const dismissed = logical.get(sendId);
      if (dismissed?.dismissed) void activeRepository.persistDismissal(tabId, dismissed);
      else if (prior.dismissed) void activeRepository.persistDismissal(tabId, prior);
      else void activeRepository.remove(tabId, sendId, prior);
    }
  }
  for (const [sendId, send] of logical) {
    if (!before.has(sendId) && !after.has(sendId) && send.dismissed) {
      void activeRepository.persistDismissal(tabId, send);
    }
  }
}

function applyPersistedRecord(tabId: string, sendId: string, record?: ChatSendDurableRecord): void {
  const current = sendsByTab.get(tabId) ?? EMPTY;
  const existing = current.find((send) => send.id === sendId);
  if (record?.deleted) {
    if (!existing) return;
    setTab(tabId, retainTabSends(current.filter((send) => send.id !== sendId)));
    notify(tabId);
    return;
  }
  const persisted = record ? decodeChatSend(record) : null;
  if (!persisted) return;
  const merged = existing ? mergeChatSendRecord(existing, undefined, persisted) : persisted;
  if (existing && sameDurableSend(existing, merged)) return;
  setTab(tabId, retainTabSends(existing
    ? current.map((send) => send.id === sendId ? merged : send)
    : [...current, merged]));
  notify(tabId);
}

function onRepositoryChange(tabId: string, sendId: string): void {
  void Promise.all([activeRepository.read(tabId, sendId), activeRepository.readDismissal(tabId, sendId)])
    .then(([record, marker]) => {
      applyPersistedRecord(tabId, sendId, record);
      if (marker && rememberDismissalMarker(tabId, marker)) notify(tabId);
    });
}

export function readChatSends(tabId: string): readonly DeliverySend[] {
  touchTab(tabId);
  void ensureTabHydrated(tabId);
  return sendsByTab.get(tabId) ?? EMPTY;
}

function readChatSendSnapshot(tabId: string): ChatSendSnapshot {
  const sends = sendsByTab.get(tabId) ?? EMPTY;
  const hydration = hydrationStates.get(tabId) ?? "pending";
  const dismissalRevision = dismissalRevisionByTab.get(tabId) ?? 0;
  const current = snapshotsByTab.get(tabId);
  if (current?.sends === sends && current.hydration === hydration
    && current.dismissalRevision === dismissalRevision) return current;
  const next = { sends, hydration, dismissalRevision };
  snapshotsByTab.set(tabId, next);
  return next;
}

export function readChatSendDismissalMarkers(
  tabId: string, rows: readonly NormalizedChatMessage[],
): readonly ChatSendDismissalMarker[] {
  touchTab(tabId);
  const lookup = getDismissalLookup(tabId);
  const result = new Map<string, ChatSendDismissalMarker>();
  const recent = dismissalMarkersByTab.get(tabId);

  for (const { row, identity } of dismissalIdentities(rows)) {
    let cached = lookup.rows.get(row);
    const markers = [...(cached?.markers ?? EMPTY_DISMISSALS)];
    for (const marker of recent?.values() ?? []) {
      if (chatSendDismissalMatchesIdentity(marker, identity)) markers.push(marker);
    }
    const unique = mergeDismissalMarkers(markers);
    if (cached && cached.state !== "pending" && !sameMarkerList(cached.markers, unique)) {
      cached = { state: cached.state, markers: unique };
      lookup.rows.set(row, cached);
    }
    for (const marker of unique) result.set(marker.sendId, marker);
  }
  const ordered = [...result.values()].sort((left, right) => left.order - right.order);
  if (sameMarkerList(lookup.result, ordered)) return lookup.result;
  lookup.result = ordered;
  return lookup.result;
}

export function requestChatSendDismissalMarkers(
  tabId: string, rows: readonly NormalizedChatMessage[],
): Promise<void> {
  const lookup = getDismissalLookup(tabId);
  const identities = dismissalIdentities(rows);
  const requestedRows = new Set<NormalizedChatMessage>();
  const requested = identities.filter(({ row }) => {
    if (lookup.rows.has(row) || requestedRows.has(row)) return false;
    requestedRows.add(row);
    return true;
  });
  if (requested.length > 0) {
    const currentLookup = lookup;
    const sourceRepository = activeRepository;
    const task = sourceRepository.loadDismissals(tabId, requested.map(({ identity }) => identity)).then((markers) => {
      if (dismissalLookupByTab.get(tabId) !== currentLookup) return;
      let found = false;
      for (const { row, identity } of requested) {
        const matches = mergeDismissalMarkers([
          ...markers.filter((marker) => chatSendDismissalMatchesIdentity(marker, identity)),
          ...(dismissalMarkersByTab.get(tabId)?.values() ?? []),
        ].filter((marker) => chatSendDismissalMatchesIdentity(marker, identity)));
        currentLookup.rows.set(row, { state: "ready", markers: matches });
        found ||= matches.length > 0;
      }
      if (!found) return;
      dismissalRevisionByTab.set(tabId, (dismissalRevisionByTab.get(tabId) ?? 0) + 1);
      notify(tabId);
    }).catch(() => {
      if (dismissalLookupByTab.get(tabId) !== currentLookup) return;
      for (const { row } of requested) currentLookup.rows.set(row, { state: "error", markers: EMPTY_DISMISSALS });
    });
    for (const { row } of requested) lookup.rows.set(row, { state: "pending", markers: EMPTY_DISMISSALS, task });
    return task;
  }
  const pending = [...new Set(identities.flatMap(({ row }) => {
    const cached = lookup.rows.get(row);
    return cached?.state === "pending" && cached.task ? [cached.task] : [];
  }))];
  return Promise.all(pending).then(() => {});
}

function getDismissalLookup(tabId: string): ChatSendDismissalLookup {
  let lookup = dismissalLookupByTab.get(tabId);
  if (!lookup) {
    lookup = { rows: new WeakMap(), result: EMPTY_DISMISSALS };
    dismissalLookupByTab.set(tabId, lookup);
  }
  return lookup;
}

function dismissalIdentities(
  rows: readonly NormalizedChatMessage[],
): Array<{ row: NormalizedChatMessage; identity: ChatSendDismissalIdentity }> {
  const result: Array<{ row: NormalizedChatMessage; identity: ChatSendDismissalIdentity }> = [];
  for (const row of rows) {
    const identity = chatSendDismissalIdentity(row);
    if (identity) result.push({ row, identity });
  }
  return result;
}

function mergeDismissalMarkers(markers: readonly ChatSendDismissalMarker[]): ChatSendDismissalMarker[] {
  const byId = new Map<string, ChatSendDismissalMarker>();
  for (const marker of markers) {
    const current = byId.get(marker.sendId);
    const merged = mergeChatSendDismissalMarker(current, marker);
    byId.set(marker.sendId, current && sameChatSendDismissalMarker(current, merged) ? current : merged);
  }
  return [...byId.values()].sort((left, right) => left.order - right.order);
}

function sameMarkerList(left: readonly ChatSendDismissalMarker[], right: readonly ChatSendDismissalMarker[]): boolean {
  return left.length === right.length && left.every((marker, index) => {
    const candidate = right[index];
    return candidate != null && sameChatSendDismissalMarker(marker, candidate);
  });
}

export function ensureChatSendTabHydrated(tabId: string): Promise<void> {
  return ensureTabHydrated(tabId);
}

export function retryChatSendTabHydration(tabId: string): Promise<void> {
  return retryTabHydration(tabId);
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
  publish(tabId, [...readChatSends(tabId), send]);
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
  if (!send || deleting.has(deletionKey(tabId, id)) || !canRetryChatSend(send)) return;
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
  for (const tabId of hydrationTasks.keys()) {
    hydrationGenerations.set(tabId, (hydrationGenerations.get(tabId) ?? 0) + 1);
  }
  sendsByTab.clear();
  cachedTabs.clear();
  hydrationStates.clear();
  hydrationTasks.clear();
  hydrationRemovedIds.clear();
  snapshotsByTab.clear();
  dismissalMarkersByTab.clear();
  dismissalLookupByTab.clear();
  dismissalRevisionByTab.clear();
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
  const retained = retainTabSends(next);
  setTab(tabId, retained);
  persistDiff(tabId, previous, retained, next);
  notify(tabId);
  trimCachedTabs();
}

function retainTabSends(sends: readonly DeliverySend[]): readonly DeliverySend[] {
  const active = sends.filter((send) => chatSendRetention(send) === "active");
  const settled = sends.filter((send) => chatSendRetention(send) === "settled").slice(-MAX_SENDS_PER_TAB);
  const retainedIds = new Set([...active, ...settled].map((send) => send.id));
  return sends.filter((send) => retainedIds.has(send.id));
}

function subscribe(tabId: string, onChange: () => void): () => void {
  touchTab(tabId);
  const existing = listeners.get(tabId) ?? new Set<() => void>();
  existing.add(onChange);
  listeners.set(tabId, existing);
  return () => {
    existing.delete(onChange);
    if (existing.size === 0) listeners.delete(tabId);
    trimCachedTabs();
  };
}

export function useChatSends(tabId: string): readonly DeliverySend[] {
  return useChatSendSnapshot(tabId).sends;
}

export function useChatSendSnapshot(tabId: string): ChatSendSnapshot {
  const subscribeToTab = useCallback((onChange: () => void) => subscribe(tabId, onChange), [tabId]);
  const read = useCallback(() => readChatSendSnapshot(tabId), [tabId]);
  const snapshot = useSyncExternalStore(subscribeToTab, read, read);
  useEffect(() => {
    const task = hydrationStates.get(tabId) === "error"
      ? retryTabHydration(tabId)
      : ensureTabHydrated(tabId);
    void task;
  }, [tabId]);
  return snapshot;
}

export type ChatSendDismissal = { send_id: string; send: ChatSend };

export function observeChatSendDismissal(tabId: string, receipt: ChatSendDismissal): void {
  const current = readChatSends(tabId);
  const matches = (send: DeliverySend) => send.id === receipt.send.id
    || (receipt.send.correlationId != null && send.correlationId === receipt.send.correlationId);
  const matched = current.filter(matches);
  if (matched.length === 0) {
    const dismissed = { ...receipt.send, dismissed: true } as DeliverySend;
    const markerChanged = rememberDismissalMarker(tabId,
      encodeChatSendDismissalMarker(tabId, dismissed));
    if (chatSendRetention(dismissed) === "active") {
      publish(tabId, [...current, dismissed]);
      return;
    }
    if (markerChanged) {
      notify(tabId);
      void activeRepository.persistDismissal(tabId, dismissed);
    }
    return;
  }

  let markerChanged = false;
  for (const send of matched) {
    markerChanged = rememberDismissalMarker(tabId, encodeChatSendDismissalMarker(tabId, {
      ...send,
      dismissed: true,
      echo: send.echo ?? receipt.send.echo,
      matchedRow: send.matchedRow ?? receipt.send.matchedRow,
    })) || markerChanged;
  }
  const next = current.map((send) => matches(send) && !send.dismissed ? { ...send, dismissed: true,
    echo: send.echo ?? receipt.send.echo, matchedRow: send.matchedRow ?? receipt.send.matchedRow } : send);
  if (next.some((send, index) => send !== current[index])) publish(tabId, next);
  else if (markerChanged) {
    notify(tabId);
    for (const send of matched) void activeRepository.persistDismissal(tabId, send);
  }
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
