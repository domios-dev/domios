import type { ChatMsg } from "@codeterm/chat-core";
import { listen } from "../bridge";
import { IS_WEB, relayTarget, webJson } from "../webApi";

export const TRANSCRIPT_PAGE_SIZE = 24;
export const THOUGHTS_PAGE_SIZE = 16;

export type Trigger = "scout_then_concierge" | "wake_word" | "direct_batch" | "manual";
export type TranscriptTarget =
  | { kind: "detached" }
  | { kind: "attached"; epochId: string }
  | { kind: "external"; tabId: string };
export type TranscriptRange = { fromSeq: number; toSeq: number };
type Binding = { ownerTabId: string; revision: number; target: TranscriptTarget };
type Asr = { state: "ready" | "loading" | "unloaded" | "unavailable"; reason?: string };
type Scout = "ready" | "unavailable" | "disabled";
type Concierge = "ready" | "needs_attachment" | "unavailable";

export interface TranscriptRouterProviderOption {
  id: string;
  displayName: string;
  defaultModel?: string | null;
  models: string[];
}

export interface TranscriptReadiness {
  asr: Asr;
  scout: Scout;
  concierge: Concierge;
  configured?: boolean;
  defaultMicTap?: boolean;
  defaultTrigger?: Trigger;
  router?: {
    provider?: string;
    model?: string | null;
    cap?: number;
    providers?: TranscriptRouterProviderOption[];
  } | null;
}

export type RouterActivity = "idle" | "reviewing" | "stuck" | "unavailable";

export interface TranscriptOverview {
  status: {
    committedSeq: number;
    binding: Binding;
    trigger: Trigger;
    scout: Scout;
    awaitingDelivery?: boolean;
    concierge: Concierge;
    configured?: boolean;
    defaultMicTap?: boolean;
    router?: {
      provider?: string;
      model?: string | null;
      cap?: number;
      activity?: RouterActivity;
      activeTurnMs?: number | null;
    } | null;
    lastRouteError?: string | null;
  };
  cursor: { ackedSeq: number; seenSeq: number } | null;
}

export interface TranscriptRow {
  seq: number;
  atMs: number;
  content: { kind: "speech"; text: string } | { kind: "paused"; reason: string } | { kind: "resumed" };
}

export interface TranscriptHistory {
  rows: TranscriptRow[];
  gaps: TranscriptRange[];
  nextFromSeq: number | null;
}

export interface RoutedRecord {
  actionSeq: number;
  binding: Binding;
  action: {
    actionId: string;
    range: TranscriptRange;
    atMs: number;
    destination: string;
    rationale: string;
  };
}

export interface RoutedPage {
  actions: RoutedRecord[];
  nextAfterSeq: number | null;
}

export type TranscriptAttach =
  | { modeOnly: true; trigger?: Trigger; defaultMicTap?: boolean }
  | { trigger: Trigger; externalTab?: string; provider?: string; model?: string; configure?: boolean; defaultMicTap?: boolean };

export function directTranscriptAvailable(): boolean {
  return !relayTarget() && (!IS_WEB || window.isSecureContext);
}

export function createLiveTranscriptApi(owner: string) {
  function request<T>(method: "GET" | "POST", path: string, signal: AbortSignal, body?: unknown) {
    if (!directTranscriptAvailable()) throw new Error("Live Transcript needs a direct secure Domios connection.");
    return webJson<T>(method, path, body, { signal });
  }

  const base = `/api/live-transcript/${encodeURIComponent(owner)}`;
  return {
    subscribeChanges(refresh: () => void) {
      let disposed = false;
      let unlisten: (() => void) | undefined;
      void listen<{ ownerTabId?: string }>("live-transcript-changed", event => {
        if (!disposed && (!event.payload?.ownerTabId || event.payload.ownerTabId === owner)) refresh();
      }).then(cleanup => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      }).catch(() => undefined);
      return () => {
        disposed = true;
        unlisten?.();
      };
    },
    readiness: (signal: AbortSignal) => request<TranscriptReadiness>("GET", "/api/live-transcript/readiness", signal),
    overview: (signal: AbortSignal) => request<TranscriptOverview>("GET", `${base}/overview`, signal),
    history: (from: number, limit: number, signal: AbortSignal) =>
      request<TranscriptHistory>("GET", `${base}/history?fromSeq=${from}&limit=${limit}`, signal),
    events: (after: number, signal: AbortSignal) =>
      request<RoutedPage>("GET", `${base}/events?fromSeq=${after}&limit=${TRANSCRIPT_PAGE_SIZE}`, signal),
    epoch: (id: string, offset: number, signal: AbortSignal) =>
      request<{ rows: ChatMsg[]; nextOffset: number | null }>(
        "GET", `${base}/epochs/${encodeURIComponent(id)}?offset=${offset}&limit=${THOUGHTS_PAGE_SIZE}`, signal,
      ),
    attach: (binding: TranscriptAttach, signal: AbortSignal) => request("POST", `${base}/attach`, signal, binding),
    route: (range: TranscriptRange, signal: AbortSignal) => request("POST", `${base}/route`, signal, range),
    save: (title: string, body: string, signal: AbortSignal) => request("POST", "/api/mem/items", signal, {
      kind: "fact", title, body, tags: ["live-transcript"], skip_extract: true,
    }),
  };
}

export type LiveTranscriptApi = ReturnType<typeof createLiveTranscriptApi>;
