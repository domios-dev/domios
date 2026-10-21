import { useCallback, useMemo, useSyncExternalStore } from "react";
import {
  createTranscriptState,
  matchesMatchedRow,
  reduceTranscript,
  type TranscriptBatch,
  type TranscriptOutcome,
  type TranscriptState,
} from "@codeterm/chat-core";
import type { ChatSendStatus } from "./chatSendStatus";
import { frameNotifications } from "./frameNotifications";
import type { KnownChatWindow } from "./chatSessionAuthority";

export interface TranscriptWindow {
  transcript: TranscriptState;
  anchorKey: string | null;
  snapshotBinding?: string;
}

/** The only committed-history store. It outlives the hook so a Terminal <-> Chat
 *  switch restores the same window instead of building a second copy. */
const windows = new Map<string, TranscriptWindow>();
const knownWindows = new Map<string, KnownChatWindow>();
const listeners = new Map<string, Set<() => void>>();
const MAX_TRANSCRIPT_WINDOWS = 8;
const MAX_TRANSCRIPT_MEMOS = 256;

const EMPTY_WINDOW: TranscriptWindow = { transcript: createTranscriptState(), anchorKey: null };
const notifications = frameNotifications((key) => {
  for (const listener of listeners.get(key) ?? []) listener();
}, {
  request: (callback) => globalThis.requestAnimationFrame(callback),
  cancel: (handle) => globalThis.cancelAnimationFrame(handle),
});

export function transcriptWindowKey(tabId: string, agentSessionId: string | null): string {
  return `${tabId}:${agentSessionId ?? ""}`;
}

export function readTranscriptWindow(key: string): TranscriptWindow {
  return windows.get(key) ?? EMPTY_WINDOW;
}

export function rememberTranscriptWindow(key: string, knownWindow: KnownChatWindow): void {
  knownWindows.delete(key);
  knownWindows.set(key, knownWindow);
  if (knownWindows.size > MAX_TRANSCRIPT_MEMOS) knownWindows.delete(knownWindows.keys().next().value!);
}

export function readTranscriptMemo(key: string): KnownChatWindow | undefined {
  const known = knownWindows.get(key);
  if (known) rememberTranscriptWindow(key, known);
  return known;
}

export function retainedTranscriptMemo(key: string): KnownChatWindow | undefined {
  return windows.get(key)?.snapshotBinding ? readTranscriptMemo(key) : undefined;
}

export function forgetTranscriptWindowCursor(key: string): void {
  knownWindows.delete(key);
}

export function markTranscriptSnapshotReady(key: string, nonce: string | null | undefined): void {
  const current = readTranscriptWindow(key);
  const snapshotBinding = `${key}|${nonce ?? ""}`;
  if (current.snapshotBinding !== snapshotBinding) publish(key, { ...current, snapshotBinding });
}

function evictUnsubscribedWindows(protectedKey?: string): void {
  for (const key of windows.keys()) {
    if (windows.size <= MAX_TRANSCRIPT_WINDOWS) break;
    if (key === protectedKey || (listeners.get(key)?.size ?? 0) > 0) continue;
    windows.delete(key);
    notifications.remove(key);
  }
}

function publish(key: string, next: TranscriptWindow, coalesce = false): void {
  windows.delete(key);
  windows.set(key, next);
  evictUnsubscribedWindows(key);
  if (coalesce && typeof globalThis.requestAnimationFrame === "function") notifications.enqueue(key);
  else notifications.immediate(key);
}

/** Reduce one batch into the stored window and report what the reducer decided;
 *  `recover_gap` / `rotated` are the recovery signals. */
export function commitTranscriptBatch(key: string, batch: TranscriptBatch): TranscriptOutcome {
  const current = readTranscriptWindow(key);
  const { state, outcome } = reduceTranscript(current.transcript, batch);
  if (state !== current.transcript) publish(key, { ...current, transcript: state }, batch.kind === "delta");
  return outcome;
}

export function stampConfirmedTranscriptRow(key: string, status: ChatSendStatus): boolean {
  const matchedRow = status.matched_row;
  if (status.state !== "delivered" || !status.correlationId || !matchedRow) return false;
  const current = readTranscriptWindow(key);
  if (current.transcript.authority?.sourceSessionId !== matchedRow.source.source_session_id) return false;
  let found = false;
  let changed = false;
  const segments = current.transcript.segments.map((segment) => ({
    ...segment,
    rows: segment.rows.map((row) => {
      if (!matchesMatchedRow(row, matchedRow)) return row;
      found = true;
      if (row.correlationId != null && row.correlationId !== status.correlationId) return row;
      if (row.correlationId === status.correlationId) return row;
      changed = true;
      return { ...row, correlationId: status.correlationId };
    }),
  }));
  if (!found) return false;
  if (changed) publish(key, { ...current, transcript: { ...current.transcript, segments } });
  return changed || segments.some((segment) => segment.rows.some((row) =>
    matchesMatchedRow(row, matchedRow) && row.correlationId === status.correlationId));
}

export function setTranscriptAnchor(key: string, anchorKey: string | null): void {
  const current = readTranscriptWindow(key);
  if (current.anchorKey === anchorKey) return;
  publish(key, { ...current, anchorKey });
}

export function resetTranscriptWindow(key: string, preserveAnchor = false): void {
  knownWindows.delete(key);
  if (!windows.has(key)) return;
  const current = readTranscriptWindow(key);
  publish(key, {
    transcript: createTranscriptState(),
    anchorKey: preserveAnchor ? current.anchorKey : null,
  });
}

export function reconcileTranscriptWindow(
  key: string,
  batch: TranscriptBatch,
): TranscriptOutcome {
  const current = readTranscriptWindow(key);
  if (batch.kind === "snapshot" && batch.placement !== "oldest") knownWindows.delete(key);
  const { state, outcome } = reduceTranscript(current.transcript, batch);
  publish(key, {
    ...current,
    transcript: state,
    anchorKey: outcome === "rotated" ? null : current.anchorKey,
  });
  return outcome;
}

export function clearTranscriptWindows(): void {
  knownWindows.clear();
  notifications.clear();
  for (const key of [...windows.keys()]) {
    windows.delete(key);
    for (const listener of listeners.get(key) ?? []) listener();
  }
}

function subscribe(key: string, onChange: () => void): () => void {
  const existing = listeners.get(key) ?? new Set<() => void>();
  existing.add(onChange);
  listeners.set(key, existing);
  return () => {
    existing.delete(onChange);
    if (existing.size === 0) {
      listeners.delete(key);
      evictUnsubscribedWindows();
    }
  };
}

const IGNORE_CHANGE = () => {};

/** A held view keeps its subscription, so the window is never evicted, but
 *  renders the window it last saw until it is live again. */
export function useTranscriptWindow(key: string, live = true): TranscriptWindow {
  const held = useMemo(() => live ? null : readTranscriptWindow(key), [key, live]);
  const subscribeToKey = useCallback(
    (onChange: () => void) => subscribe(key, held ? IGNORE_CHANGE : onChange),
    [key, held],
  );
  const read = useCallback(() => held ?? readTranscriptWindow(key), [key, held]);
  return useSyncExternalStore(subscribeToKey, read, read);
}
