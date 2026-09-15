import { useCallback, useSyncExternalStore } from "react";
import type { ChatSend, SendEcho } from "@codeterm/chat-core";

const STORAGE_KEY = "codeterm:chat-sends:v1";
const MAX_SENDS_PER_TAB = 64;
const MAX_TABS = 32;
const EMPTY: readonly ChatSend[] = [];

/** Sends outlive the chat hook so away/back and remount keep each send's key; reconciled
 *  sends are also stored so the key their echo inherits survives a restart. */
const sendsByTab = new Map<string, readonly ChatSend[]>();
const listeners = new Map<string, Set<() => void>>();
let loaded = false;

function storage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

function isSendEcho(value: unknown): value is SendEcho {
  if (!value || typeof value !== "object") return false;
  const echo = value as Record<string, unknown>;
  return typeof echo.content === "string"
    && (echo.timestampMs === null || Number.isFinite(echo.timestampMs));
}

function isStoredSend(value: unknown): value is ChatSend {
  if (!value || typeof value !== "object") return false;
  const send = value as Record<string, unknown>;
  return typeof send.id === "string"
    && typeof send.content === "string"
    && Number.isFinite(send.sentAtMs)
    && (send.correlationId === undefined || typeof send.correlationId === "string")
    && isSendEcho(send.echo);
}

function load(): void {
  if (loaded) return;
  loaded = true;
  try {
    const raw = storage()?.getItem(STORAGE_KEY);
    if (!raw) return;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return;
    for (const [tabId, list] of Object.entries(parsed as Record<string, unknown>)) {
      if (Array.isArray(list)) sendsByTab.set(tabId, list.filter(isStoredSend));
    }
  } catch {
    // Unreadable storage only loses keys of reconciled sends; their echoes fall back to source time.
  }
}

function save(): void {
  const stored: Record<string, ChatSend[]> = {};
  for (const [tabId, list] of sendsByTab) {
    const reconciled = list.filter((send) => send.echo != null);
    if (reconciled.length > 0) stored[tabId] = reconciled;
  }
  try {
    storage()?.setItem(STORAGE_KEY, JSON.stringify(stored));
  } catch {
    // Quota or private mode never blocks a send.
  }
}

function publish(tabId: string, next: readonly ChatSend[]): void {
  sendsByTab.delete(tabId);
  sendsByTab.set(tabId, next);
  while (sendsByTab.size > MAX_TABS) {
    const oldest = sendsByTab.keys().next().value;
    if (oldest == null || oldest === tabId) break;
    sendsByTab.delete(oldest);
  }
  save();
  for (const listener of listeners.get(tabId) ?? []) listener();
}

export function readChatSends(tabId: string): readonly ChatSend[] {
  load();
  return sendsByTab.get(tabId) ?? EMPTY;
}

export function addChatSend(tabId: string, send: ChatSend): void {
  publish(tabId, [...readChatSends(tabId), send].slice(-MAX_SENDS_PER_TAB));
}

/** Content may morph (a transcription placeholder); the send's key never does. */
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
    return send.echo != null || echo == null ? send : { ...send, echo };
  }));
}

export function chatSendAt(tabId: string, id: string | undefined): number | null {
  if (!id) return null;
  return readChatSends(tabId).find((send) => send.id === id)?.sentAtMs ?? null;
}

/** Drops the in-memory copy exactly as a restart does; stored reconciled sends reload on next read. */
export function forgetChatSendsInMemory(): void {
  sendsByTab.clear();
  loaded = false;
  for (const set of listeners.values()) for (const listener of set) listener();
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

export function useChatSends(tabId: string): readonly ChatSend[] {
  const subscribeToTab = useCallback((onChange: () => void) => subscribe(tabId, onChange), [tabId]);
  const read = useCallback(() => readChatSends(tabId), [tabId]);
  return useSyncExternalStore(subscribeToTab, read, read);
}
