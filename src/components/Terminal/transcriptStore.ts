import { useCallback, useSyncExternalStore } from "react";
import {
  createTranscriptState,
  reduceTranscript,
  type TranscriptBatch,
  type TranscriptOutcome,
  type TranscriptState,
} from "@codeterm/chat-core";

export interface TranscriptWindow {
  transcript: TranscriptState;
  anchorKey: string | null;
}

/** The only committed-history store. It outlives the hook so a Terminal <-> Chat
 *  switch restores the same window instead of building a second copy. */
const windows = new Map<string, TranscriptWindow>();
const listeners = new Map<string, Set<() => void>>();
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

export function setTranscriptAnchor(key: string, anchorKey: string | null): void {
  const current = readTranscriptWindow(key);
  if (current.anchorKey === anchorKey) return;
  publish(key, { ...current, anchorKey });
}

export function resetTranscriptWindow(key: string, preserveAnchor = false): void {
  if (!windows.has(key)) return;
  const current = readTranscriptWindow(key);
  publish(key, {
    transcript: createTranscriptState(),
    anchorKey: preserveAnchor ? current.anchorKey : null,
  });
}

export function clearTranscriptWindows(): void {
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
