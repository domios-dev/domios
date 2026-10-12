import type { SseMessage } from "./sseParser";
import type { FrameMetadata, StreamViewport } from "./streamModel";
import type { SurfaceState } from "./surfaceChromeModel";
import { NOTICE_KIND, type StreamFrame, type SurfaceNotice, type SurfaceNoticeKind } from "./surfaceStreamTransport";

export type SurfaceStreamEvent =
  | { type: "state"; state: SurfaceState }
  | { type: "frame"; frame: StreamFrame }
  | { type: "notice"; notice: SurfaceNotice };

export type CallReply<T> = { ok: true; result: T } | { ok: false; message: string };

export type StreamOutcome =
  | { kind: "refused"; message: string }
  | { kind: "dropped"; delivered: boolean; message: string };

export interface ReconnectState {
  failures: number;
  parked: boolean;
  stopped: boolean;
  announced: boolean;
}

export type ReconnectAction = "retry" | "park" | "stop";

export interface ReconnectStep {
  action: ReconnectAction;
  state: ReconnectState;
  notice?: SurfaceNotice;
}

export const MAX_IMMEDIATE_RECONNECTS = 3;
export const INITIAL_RECONNECT: ReconnectState = { failures: 0, parked: false, stopped: false, announced: false };

function surfacePath(base: string, tabId: string, leaf: string): string {
  return `${base.replace(/\/+$/, "")}/api/surfaces/${encodeURIComponent(tabId)}/${leaf}`;
}

export function surfaceStreamUrl(base: string, tabId: string, viewport: StreamViewport, peerId?: string): string {
  const query = new URLSearchParams({ w: String(viewport.w), h: String(viewport.h), dpr: String(viewport.dpr) });
  const path = `/api/surfaces/${encodeURIComponent(tabId)}/stream?${query.toString()}`;
  if (peerId) {
    const relay = new URLSearchParams({ peer: peerId, path });
    return `${base.replace(/\/+$/, "")}/api/mesh/relay-stream?${relay}`;
  }
  return `${base.replace(/\/+$/, "")}${path}`;
}

export function surfaceCallUrl(base: string, tabId: string): string {
  return surfacePath(base, tabId, "call");
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

function optionalString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function toState(value: Record<string, unknown>): SurfaceState {
  const state: SurfaceState = { owner: optionalString(value.owner) ?? "" };
  if (typeof value.targetId === "string") state.targetId = value.targetId;
  if (typeof value.title === "string") state.title = value.title;
  if (typeof value.url === "string") state.url = value.url;
  if (typeof value.activity === "string" || value.activity === null) state.activity = value.activity;
  if (typeof value.needsHuman === "string" || value.needsHuman === null) state.needsHuman = value.needsHuman as SurfaceState["needsHuman"];
  if (typeof value.profile === "string" || value.profile === null) state.profile = value.profile;
  if (Array.isArray(value.profiles)) state.profiles = value.profiles.filter((name): name is string => typeof name === "string");
  return state;
}

function toFrame(value: Record<string, unknown>): StreamFrame | null {
  const metadata = value.metadata;
  if (typeof value.data !== "string" || !isRecord(metadata)) return null;
  if (typeof metadata.deviceWidth !== "number" || typeof metadata.deviceHeight !== "number") return null;
  return { data: value.data, metadata: metadata as unknown as FrameMetadata };
}

const NOTICE_KINDS: readonly unknown[] = Object.values(NOTICE_KIND);

function toNotice(value: Record<string, unknown>): SurfaceNotice | null {
  if (typeof value.text !== "string") return null;
  const notice: SurfaceNotice = { text: value.text };
  if (value.tone === "error" || value.tone === "info") notice.tone = value.tone;
  if (NOTICE_KINDS.includes(value.kind)) notice.kind = value.kind as SurfaceNoticeKind;
  if (typeof value.detail === "string") notice.detail = value.detail;
  return notice;
}

export function surfaceStreamEvent(message: SseMessage): SurfaceStreamEvent | null {
  const value = parseJson(message.data);
  if (!isRecord(value)) return null;
  switch (message.event) {
    case "state":
      return { type: "state", state: toState(value) };
    case "frame": {
      const frame = toFrame(value);
      return frame ? { type: "frame", frame } : null;
    }
    case "notice": {
      const notice = toNotice(value);
      return notice ? { type: "notice", notice } : null;
    }
    default:
      return null;
  }
}

function errorText(error: unknown): string | null {
  if (typeof error === "string") return error;
  if (isRecord(error)) return optionalString(error.message) ?? optionalString(error.code) ?? null;
  return null;
}

export function replyErrorMessage(status: number, body: string): string {
  const value = parseJson(body);
  const fromBody = isRecord(value) ? errorText(value.error) : null;
  if (fromBody) return fromBody;
  const raw = body.trim();
  return raw ? `HTTP ${status}: ${raw.slice(0, 200)}` : `HTTP ${status}`;
}

export function callReply<T>(status: number, body: string): CallReply<T> {
  if (status < 200 || status >= 300) return { ok: false, message: replyErrorMessage(status, body) };
  const value = parseJson(body);
  if (!isRecord(value)) return { ok: false, message: "Malformed surface reply" };
  const error = errorText(value.error);
  if (error) return { ok: false, message: error };
  return { ok: true, result: (value.result ?? null) as T };
}

// 408 and 429 are transient; every other 4xx (409 not a stream surface, 401, 404) will not heal by retrying.
export function streamOutcomeForStatus(status: number, body: string): StreamOutcome {
  const message = replyErrorMessage(status, body);
  if (status >= 400 && status < 500 && status !== 408 && status !== 429) return { kind: "refused", message };
  return { kind: "dropped", delivered: false, message };
}

export function disconnectedNotice(reason: string): SurfaceNotice {
  return { text: `Live view disconnected: ${reason}. It reconnects when Domios is reachable again.`, tone: "error" };
}

export function refusedNotice(reason: string): SurfaceNotice {
  return { text: `Live view unavailable: ${reason}`, tone: "error" };
}

export const RECONNECTED_NOTICE: SurfaceNotice = { text: "Live view reconnected", tone: "info" };

export function nextReconnect(state: ReconnectState, outcome: StreamOutcome, maxFailures = MAX_IMMEDIATE_RECONNECTS): ReconnectStep {
  if (outcome.kind === "refused") {
    return { action: "stop", state: { ...state, stopped: true, parked: false, announced: true }, notice: refusedNotice(outcome.message) };
  }
  const failures = outcome.delivered ? 1 : state.failures + 1;
  if (failures <= maxFailures) return { action: "retry", state: { ...state, failures, parked: false } };
  const step: ReconnectStep = { action: "park", state: { ...state, failures, parked: true, announced: true } };
  if (!state.announced) step.notice = disconnectedNotice(outcome.message);
  return step;
}

export function reconnectOnSignal(state: ReconnectState): { retry: boolean; state: ReconnectState } {
  if (!state.parked || state.stopped) return { retry: false, state };
  return { retry: true, state: { ...state, parked: false, failures: 0 } };
}

export function streamOpened(state: ReconnectState): { state: ReconnectState; notice?: SurfaceNotice } {
  const next = { ...state, parked: false };
  if (!state.announced) return { state: next };
  return { state: { ...next, announced: false }, notice: RECONNECTED_NOTICE };
}
