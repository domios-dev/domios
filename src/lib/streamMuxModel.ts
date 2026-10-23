import { TransportError, type StreamEnd } from "./transportClient";

const MUX_STREAM_PATHS = new Set(["/api/mesh/relay-stream", "/api/output/stream"]);

/** Long-lived streams that ride the shared multiplexed connection instead of their own socket. */
export function isMuxStreamPath(path: string): boolean {
  if (!path.startsWith("/") || path.startsWith("//")) return false;
  return MUX_STREAM_PATHS.has(new URL(path, "http://daemon").pathname);
}

export type MuxChannelEnd =
  | { reason: "eof" }
  | { reason: "http"; status: number; body: string }
  | { reason: "error"; message: string }
  | { reason: "capacity"; bytes: number };

export type MuxFrame =
  | { channel: string; kind: "data"; event: string; data: unknown }
  | { channel: string; kind: "activity" }
  | { channel: string; kind: "open"; contentType: string | null }
  | { channel: string; kind: "end"; end: MuxChannelEnd };

function channelEnd(value: unknown): MuxChannelEnd | null {
  if (typeof value !== "object" || value === null || !("reason" in value)) return null;
  const end = value as Record<string, unknown>;
  switch (end.reason) {
    case "eof": return { reason: "eof" };
    case "http": return typeof end.status === "number" ? { reason: "http", status: end.status, body: String(end.body ?? "") } : null;
    case "error": return { reason: "error", message: String(end.message ?? "") };
    case "capacity": return typeof end.bytes === "number" ? { reason: "capacity", bytes: end.bytes } : null;
    default: return null;
  }
}

export function decodeMuxFrame(value: unknown): MuxFrame | null {
  if (typeof value !== "object" || value === null) return null;
  const frame = value as Record<string, unknown>;
  if (typeof frame.channel !== "string" || !frame.channel) return null;
  const { channel } = frame;
  if ("end" in frame) {
    const end = channelEnd(frame.end);
    return end ? { channel, kind: "end", end } : null;
  }
  if (frame.open === true) return { channel, kind: "open", contentType: typeof frame.content_type === "string" ? frame.content_type : null };
  if ("data" in frame) return { channel, kind: "data", event: typeof frame.event === "string" ? frame.event : "message", data: frame.data };
  if (frame.activity === true) return { channel, kind: "activity" };
  return null;
}

export function muxStreamEnd(end: MuxChannelEnd, path: string): StreamEnd {
  switch (end.reason) {
    case "eof": return { reason: "eof" };
    case "http": return { reason: "http", status: end.status, error: new TransportError(end.status, path, end.body) };
    case "error": return { reason: "error", error: new Error(end.message) };
    case "capacity": return { reason: "capacity", bytes: end.bytes };
  }
}

export type MuxStreamPhase = "closed" | "opening" | "ready";
export type MuxChannelPhase = "pending" | "requested";

export interface MuxChannelSet {
  readonly stream: MuxStreamPhase;
  readonly channels: ReadonlyMap<string, MuxChannelPhase>;
}

export type MuxEffect =
  | { type: "open-stream" }
  | { type: "close-stream" }
  | { type: "request-channel"; id: string }
  | { type: "release-channel"; id: string };

export interface MuxTransition {
  state: MuxChannelSet;
  effects: MuxEffect[];
}

export const EMPTY_MUX_CHANNEL_SET: MuxChannelSet = { stream: "closed", channels: new Map() };

export function muxAddChannel(state: MuxChannelSet, id: string): MuxTransition {
  const channels = new Map(state.channels);
  if (state.stream === "ready") {
    channels.set(id, "requested");
    return { state: { stream: "ready", channels }, effects: [{ type: "request-channel", id }] };
  }
  channels.set(id, "pending");
  return state.stream === "closed"
    ? { state: { stream: "opening", channels }, effects: [{ type: "open-stream" }] }
    : { state: { stream: state.stream, channels }, effects: [] };
}

/** `serverClosed` marks a channel the daemon already ended, so it needs no release request. */
export function muxRemoveChannel(state: MuxChannelSet, id: string, serverClosed = false): MuxTransition {
  const phase = state.channels.get(id);
  if (!phase) return { state, effects: [] };
  const channels = new Map(state.channels);
  channels.delete(id);
  if (channels.size === 0) return { state: EMPTY_MUX_CHANNEL_SET, effects: [{ type: "close-stream" }] };
  const effects: MuxEffect[] = phase === "requested" && !serverClosed ? [{ type: "release-channel", id }] : [];
  return { state: { stream: state.stream, channels }, effects };
}

export function muxStreamReady(state: MuxChannelSet): MuxTransition {
  if (state.stream !== "opening") return { state, effects: [] };
  const channels = new Map<string, MuxChannelPhase>();
  const effects: MuxEffect[] = [];
  for (const [id, phase] of state.channels) {
    channels.set(id, "requested");
    if (phase === "pending") effects.push({ type: "request-channel", id });
  }
  return { state: { stream: "ready", channels }, effects };
}

/** The shared connection ended: every channel ends with it and the set starts empty. */
export function muxStreamEnded(state: MuxChannelSet): { state: MuxChannelSet; ended: string[] } {
  return { state: EMPTY_MUX_CHANNEL_SET, ended: [...state.channels.keys()] };
}
