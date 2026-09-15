import { useCallback, useSyncExternalStore } from "react";
import {
  ECHO_SKEW_MS,
  createTranscriptState,
  reduceTranscript,
  timestampMs,
  type TranscriptBatch,
  type TranscriptOutcome,
  type TranscriptState,
} from "@codeterm/chat-core";
import type { ChatSendStatus } from "./chatSendStatus";

export interface TranscriptWindow {
  transcript: TranscriptState;
  anchorKey: string | null;
}

/** The only committed-history store. It outlives the hook so a Terminal <-> Chat
 *  switch restores the same window instead of building a second copy. */
const windows = new Map<string, TranscriptWindow>();
const listeners = new Map<string, Set<() => void>>();
const pendingSends = new Map<string, Map<string, { correlationId: string; sentAtMs: number }>>();
const MAX_TRANSCRIPT_WINDOWS = 8;

const EMPTY_WINDOW: TranscriptWindow = { transcript: createTranscriptState(), anchorKey: null };

export function transcriptWindowKey(tabId: string, agentSessionId: string | null): string {
  return `${tabId}:${agentSessionId ?? ""}`;
}

export function readTranscriptWindow(key: string): TranscriptWindow {
  return windows.get(key) ?? EMPTY_WINDOW;
}

function publish(key: string, next: TranscriptWindow): void {
  windows.delete(key);
  windows.set(key, next);
  while (windows.size > MAX_TRANSCRIPT_WINDOWS) {
    const oldest = windows.keys().next().value;
    if (oldest == null || oldest === key) break;
    windows.delete(oldest);
  }
  for (const listener of listeners.get(key) ?? []) listener();
}

/** Reduce one batch into the stored window and report what the reducer decided;
 *  `recover_gap` / `recover_overlap` / `rotated` are the recovery signals. */
export function commitTranscriptBatch(key: string, batch: TranscriptBatch): TranscriptOutcome {
  const current = readTranscriptWindow(key);
  const { state, outcome } = reduceTranscript(current.transcript, batch);
  if (state !== current.transcript) publish(key, { ...current, transcript: state });
  return outcome;
}

/** `sentAtMs` is the send's own key; only a user row stamped at or after it can echo it. */
export function trackTranscriptSend(key: string, status: ChatSendStatus, sentAtMs: number | null): void {
  const correlationId = status.correlationId;
  if (!correlationId || sentAtMs == null) return;
  const sends = pendingSends.get(key) ?? new Map();
  if (!sends.has(status.send_id)) {
    sends.set(status.send_id, { correlationId, sentAtMs });
    pendingSends.set(key, sends);
  }
  if (status.state === "confirmed") stampConfirmedTranscriptRow(key, status);
}

export function stampConfirmedTranscriptRow(key: string, status: ChatSendStatus): boolean {
  if (status.state !== "confirmed") return false;
  const sends = pendingSends.get(key);
  const pending = sends?.get(status.send_id);
  const correlationId = status.correlationId ?? pending?.correlationId;
  if (!pending || !correlationId) return false;
  const current = readTranscriptWindow(key);
  let stamped = false;
  const segments = current.transcript.segments.map((segment) => ({
    ...segment,
    rows: segment.rows.map((row) => {
      const ts = timestampMs(row.timestamp);
      if (
        stamped
        || row.correlationId != null
        || (row.type !== "user" && row.type !== "local_command")
        || ts == null
        || ts < pending.sentAtMs - ECHO_SKEW_MS
      ) return row;
      stamped = true;
      return { ...row, correlationId };
    }),
  }));
  if (!stamped) return false;
  sends?.delete(status.send_id);
  if (sends?.size === 0) pendingSends.delete(key);
  publish(key, { ...current, transcript: { ...current.transcript, segments } });
  return true;
}

export function setTranscriptAnchor(key: string, anchorKey: string | null): void {
  const current = readTranscriptWindow(key);
  if (current.anchorKey === anchorKey) return;
  publish(key, { ...current, anchorKey });
}

export function resetTranscriptWindow(key: string, preserveAnchor = false): void {
  pendingSends.delete(key);
  if (!windows.has(key)) return;
  const current = readTranscriptWindow(key);
  publish(key, {
    transcript: createTranscriptState(),
    anchorKey: preserveAnchor ? current.anchorKey : null,
  });
}

export function replaceTranscriptWindow(
  key: string,
  batch: TranscriptBatch,
  preserveAnchor: boolean,
): TranscriptOutcome {
  const current = readTranscriptWindow(key);
  const initial = createTranscriptState();
  const { state, outcome } = reduceTranscript(initial, batch);
  publish(key, {
    transcript: batch.rows.length === 0 ? initial : state,
    anchorKey: preserveAnchor ? current.anchorKey : null,
  });
  return outcome;
}

export function clearTranscriptWindows(): void {
  pendingSends.clear();
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
    if (existing.size === 0) listeners.delete(key);
  };
}

export function useTranscriptWindow(key: string): TranscriptWindow {
  const subscribeToKey = useCallback(
    (onChange: () => void) => subscribe(key, onChange),
    [key],
  );
  const read = useCallback(() => readTranscriptWindow(key), [key]);
  return useSyncExternalStore(subscribeToKey, read, read);
}
