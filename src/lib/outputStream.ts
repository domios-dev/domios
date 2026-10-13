import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "./terminalViewerEvents";
import { createAggregateOutputRegistry } from "./aggregateOutputRegistry";
import { OutputRetention } from "./outputRetention";
import { apiHost, currentToken, relayTarget } from "./webApi";
import { onResync } from "./resync";
import {
  onRemoteTransportCredentials,
  parseCompositeTabId,
  remoteTabTransportHttp,
} from "./remoteTabTransport";
import { remoteTabStreamPath, tabOutputRoute } from "./remoteTabRoute";
import { publishRemoteTransportState } from "./remoteTabTransportState";
import {
  createRemoteChannelRecovery, createSilenceDeadline, streamResponseFailureReason,
  type RemoteChannelRecovery, type StreamEndReason,
} from "./remoteChannelRecovery";
import { createTransportClient, type TransportClient } from "./transportClient";
import { TerminalOutputReplay } from "./outputReplay";
import { planOutputResync } from "./outputResync";
import { DEV_BUILD } from "./devBuild";

function selectedRelayTarget() {
  return typeof window === "undefined" || !window.location || typeof localStorage === "undefined" ? null : relayTarget();
}

let outputClient: TransportClient | undefined;
export function bindOutputStreamTransport(client: TransportClient): void {
  outputClient = client;
  resetOutputStream();
  for (const [tabId, record] of remoteRecords) {
    record.controller?.abort();
    openRemoteOutputConnection(tabId, record);
  }
}

export interface TabOutputSink {
  onOutput(text: string): void;
  onReady?(): void;
  onBytes?(bytes: Uint8Array): void;
  onTruncated(from: number): void;
  onEnded(): void;
  onStreamError?(error: Error): void;
}

interface OutputFrame {
  tab_id?: string;
  output?: string;
  data?: number[];
  pty_id?: string;
  offset?: number;
  truncated?: boolean;
  from?: number;
  ended?: boolean;
  boot_changed?: boolean;
  boot?: string;
}

const BOOT_KEY = "codeterm-web-output-boot";
const replay = new TerminalOutputReplay();
const retainedOutput = new OutputRetention();
const aggregate = createAggregateOutputRegistry(openAggregateStream);
const sinkIdentities = new WeakMap<TabOutputSink, { terminal: object; ptyId: string }>();

declare global {
  interface Window {
    __outputStreamPerf?: {
      frames: number;
      bytes: number;
      batches: number;
      lastFrameAt: number;
    };
  }
}

const sinks = new Map<string, Set<TabOutputSink>>();
const offsets = new Map<string, number>();
const rings = new Map<string, { boot: string; ptyId?: string }>();

let boot: string | null = readBoot();
let streamAbort: AbortController | null = null;
let started = false;

export function replayOffsetsAfterBootChange(tabIds: Iterable<string>): Map<string, number> {
  return new Map<string, number>([...tabIds].map((tabId): [string, number] => [tabId, 0]));
}

function readBoot(): string | null {
  try {
    return sessionStorage.getItem(BOOT_KEY);
  } catch {
    return null;
  }
}

function saveBoot(b: string): void {
  boot = b;
  try {
    sessionStorage.setItem(BOOT_KEY, b);
  } catch {
    /* private mode — memory-only boot still works */
  }
}

export function outputStreamEnabled(): boolean { return true; }

export function webOutputStreamActive(): boolean { return true; }

export function webOutputTransportActive(_isWeb: boolean, _relayPeerId: string | null, _enabled: boolean): boolean {
  return true;
}

function sinceCursor(): string {
  const tabs: Record<string, number> = {};
  for (const [tab, off] of offsets) tabs[tab] = off;
  const json = JSON.stringify({ boot: boot ?? "", tabs });
  return btoa(String.fromCharCode(...new TextEncoder().encode(json)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function outputStreamUrl(): string {
  const cursor = sinceCursor();
  const url = `${outputClient?.origin() ?? apiHost()}/api/output/stream?since=${cursor}`;
  if (DEV_BUILD) console.debug("[output-stream] local aggregate stream", { url });
  return url;
}

function handleFrame(msg: OutputFrame): void {
  const perf = window.__outputStreamPerf;
  if (perf) {
    perf.frames += 1;
    perf.lastFrameAt = performance.now();
    if (typeof msg.output === "string") perf.bytes += msg.output.length;
  }
  if (msg.boot_changed && msg.boot) {
    // boot-scoped offsets and reconnect — the boot-matched cursor full-replays.
    saveBoot(msg.boot);
    offsets.clear();
    rings.clear();
    retainedOutput.clear();
    for (const [tabId, offset] of replayOffsetsAfterBootChange(sinks.keys())) {
      offsets.set(tabId, offset);
    }
    queueMicrotask(() => aggregate.replace(openAggregateStream));
    return;
  }
  if (msg.boot) saveBoot(msg.boot);
  const tab = msg.tab_id;
  if (!tab) return;
  const set = sinks.get(tab);
  if (typeof msg.offset === "number" && typeof msg.output === "string") {
    const ring = { boot: msg.boot ?? boot ?? "", ptyId: msg.pty_id };
    const previous = rings.get(tab);
    const sameRing = previous?.boot === ring.boot && previous.ptyId === ring.ptyId;
    offsets.set(tab, sameRing ? Math.max(offsets.get(tab) ?? 0, msg.offset) : msg.offset);
    rings.set(tab, ring);
    retainedOutput.append(tab, {
      boot: ring.boot, ptyId: msg.pty_id ?? tab, offset: msg.offset,
      bytes: msg.data ? new Uint8Array(msg.data) : new TextEncoder().encode(msg.output),
    });
    if (set) {
      perf && (perf.batches += 1);
      for (const sink of set) deliverOutput(tab, sink, msg);
    }
    return;
  }
  if (msg.truncated && typeof msg.from === "number") {
    offsets.set(tab, planOutputResync(offsets.get(tab), msg.from).offset);
    if (set) for (const sink of set) deliverTruncation(tab, sink, msg.from, boot ?? "");
    return;
  }
  if (msg.ended) {
    if (set) for (const s of set) s.onEnded();
  }
}

function deliverTruncation(tabId: string, sink: TabOutputSink, from: number, bootId: string): void {
  const identity = sinkIdentities.get(sink) ?? { terminal: sink, ptyId: tabId };
  replay.resync(identity.terminal, tabId, bootId, identity.ptyId, from, () => sink.onTruncated(from));
}

function deliverOutput(tabId: string, sink: TabOutputSink, frame: OutputFrame): void {
  const identity = sinkIdentities.get(sink) ?? { terminal: sink, ptyId: frame.pty_id ?? tabId };
  const bytes = frame.data ? new Uint8Array(frame.data) : new TextEncoder().encode(frame.output ?? "");
  replay.deliver(identity.terminal, tabId, {
    boot: frame.boot ?? boot ?? "", ptyId: frame.pty_id ?? identity.ptyId,
    offset: frame.offset ?? bytes.length, bytes,
  }, (payload) => writeSinkBytes(sink, payload));
}

function writeSinkBytes(sink: TabOutputSink, bytes: Uint8Array): void {
  if (sink.onBytes) sink.onBytes(bytes);
  else sink.onOutput(new TextDecoder().decode(bytes));
}

function recoverOutputStream(): void {
  aggregate.recover();
}

function openAggregateStream(): () => void {
  const abort = new AbortController();
  streamAbort = abort;
  const client = outputClient ?? createTransportClient({ origin: apiHost(), token: currentToken });
  client.subscribe(outputStreamUrl().slice(client.origin().length), (_name, data) => {
    if (abort.signal.aborted || streamAbort !== abort) return;
    handleFrame(data as OutputFrame);
  }, {
    signal: abort.signal, raw: true,
    onActivity() { for (const set of sinks.values()) for (const sink of set) sink.onReady?.(); },
    onEnd(end) {
      if (abort.signal.aborted || streamAbort !== abort) return;
      streamAbort = null;
      aggregate.ended();
      if (DEV_BUILD) console.debug("[output-stream] waiting for recovery signal", { reason: end.reason });
      if (end.reason === "error" || end.reason === "http") {
        const error = end.error instanceof Error ? end.error : new Error(String(end.error));
        for (const set of sinks.values()) for (const sink of set) sink.onStreamError?.(error);
      }
    },
  });
  return () => { abort.abort(); if (streamAbort === abort) streamAbort = null; };
}

function ensureStarted(): void {
  if (started || selectedRelayTarget()) return;
  started = true;
  window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, recoverOutputStream);
  aggregate.attach();
}

export function subscribeTabOutput(tabId: string, sink: TabOutputSink, terminal: object = sink, ptyId: string = tabId): () => void {
  sinkIdentities.set(sink, { terminal, ptyId });
  const route = tabOutputRoute(tabId, selectedRelayTarget()?.id ?? null);
  if (route) return subscribeRemoteTabOutput(`mesh:${route.peerId}/${route.tabId}`, route.peerId, sink);
  let set = sinks.get(tabId);
  if (!set) {
    set = new Set();
    sinks.set(tabId, set);
  }
  set.add(sink);
  const cached = retainedOutput.snapshot(tabId);
  if (cached && cached.ptyId === ptyId) {
    const from = cached.offset - cached.bytes.length;
    deliverTruncation(tabId, sink, from, cached.boot);
    replay.deliver(terminal, tabId, cached, (bytes) => writeSinkBytes(sink, bytes));
    sink.onReady?.();
  }
  ensureStarted();
  return () => {
    set!.delete(sink);
    if (set!.size === 0) sinks.delete(tabId);
  };
}

function subscribeRemoteTabOutput(
  ordinaryTabId: string,
  peerId: string,
  sink: TabOutputSink,
): () => void {
  let set = remoteSinks.get(ordinaryTabId);
  if (!set) { set = new Set(); remoteSinks.set(ordinaryTabId, set); }
  const existing = remoteRecords.get(ordinaryTabId);
  const cursor = existing?.recovery.currentCursor();
  const identity = sinkIdentities.get(sink);
  if (existing && cursor && identity && replay.offset(identity.terminal, ordinaryTabId, cursor.boot ?? "", identity.ptyId) < cursor.offset) {
    existing.recovery.dispose();
    existing.controller?.abort();
    remoteRecords.delete(ordinaryTabId);
  }
  set.add(sink);
  startRemoteTabOutput(ordinaryTabId, peerId);
  return () => {
    set!.delete(sink);
    if (set!.size === 0) {
      remoteSinks.delete(ordinaryTabId);
      const record = remoteRecords.get(ordinaryTabId);
      record?.recovery.dispose();
      record?.controller?.abort();
      remoteRecords.delete(ordinaryTabId);
    }
  };
}

interface RemoteOutputCursor {
  boot?: string;
  ptyId?: string;
  offset: number;
}

interface RemoteOutputRecord {
  peerId: string;
  controller: AbortController | null;
  recovery: RemoteChannelRecovery<RemoteOutputCursor>;
  settlements: Map<number, { promise: Promise<void>; resolve: () => void }>;
}

const remoteSinks = new Map<string, Set<TabOutputSink>>();
const remoteRecords = new Map<string, RemoteOutputRecord>();

// Strictly-ahead cursor test: a new boot ring or a higher byte offset is forward
// progress; anything else is a frame the server replayed and we already delivered.
function remoteOutputAdvanced(prev: RemoteOutputCursor | null, next: RemoteOutputCursor): boolean {
  if (!prev) return true;
  if (next.boot !== prev.boot || next.ptyId !== prev.ptyId) return true;
  return next.offset > prev.offset;
}

export function remoteOutputSinceCursor(remoteTabId: string, bootId: string, offset: number): string {
  const raw = JSON.stringify({ boot: bootId, tabs: { [remoteTabId]: offset } });
  return btoa(raw).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function startRemoteTabOutput(ordinaryTabId: string, peerId: string): void {
  let record = remoteRecords.get(ordinaryTabId);
  if (!record) {
    record = {
      peerId,
      controller: null,
      recovery: createRemoteChannelRecovery<RemoteOutputCursor>({
        channel: "output",
        advanced: remoteOutputAdvanced,
        onAvailability: (channel, availability) =>
          publishRemoteTransportState(ordinaryTabId, channel, availability),
      }),
      settlements: new Map(),
    };
    remoteRecords.set(ordinaryTabId, record);
  }
  record.peerId = peerId;
  if (record.controller && !record.controller.signal.aborted) return;
  openRemoteOutputConnection(ordinaryTabId, record);
}

function openRemoteOutputConnection(ordinaryTabId: string, record: RemoteOutputRecord): void {
  const remoteTabId = parseCompositeTabId(ordinaryTabId)?.remoteTabId;
  const generation = record.recovery.beginConnection();
  let settle!: () => void;
  const settlement = new Promise<void>((resolve) => { settle = resolve; });
  record.settlements.set(generation, { promise: settlement, resolve: settle });
  if (!remoteTabId) {
    resolveRemoteSettlement(record, generation);
    return;
  }
  const abort = new AbortController();
  record.controller = abort;
  // Re-arm the silence deadline on every byte so a cursor-less connection cannot remain parked forever after a path drop.
  const deadline = createSilenceDeadline(record.recovery, generation, abort);
  deadline.arm();
  const cursor = record.recovery.currentCursor();
  const since = cursor?.boot ? remoteOutputSinceCursor(remoteTabId, cursor.boot, cursor.offset) : undefined;
  const path = remoteTabStreamPath({ peerId: record.peerId, tabId: remoteTabId }, since);
  if (DEV_BUILD) console.debug("[output-stream] remote tab stream", { ordinaryTabId, path, generation });
  const transport = remoteTabTransportHttp();
  const client = outputClient ?? createTransportClient({ origin: transport.base, token: () => remoteTabTransportHttp().token });
  client.subscribe(path, (_name, data) => {
    if (abort.signal.aborted || !record.recovery.isCurrent(generation)) return;
    handleRemoteOutputFrame(ordinaryTabId, record, generation, remoteTabId, data as OutputFrame);
  }, {
    signal: abort.signal, raw: true, onActivity() {
      deadline.arm();
      if (record.recovery.isCurrent(generation)) for (const sink of remoteSinks.get(ordinaryTabId) ?? []) sink.onReady?.();
    },
    onEnd(end) {
      deadline.clear();
      const reason: StreamEndReason = end.reason === "http"
        ? streamResponseFailureReason(end.status ?? 500) : end.reason === "eof" ? "eof" : "error";
      if (!abort.signal.aborted && record.recovery.isCurrent(generation) && end.error) {
        const error = end.error instanceof Error ? end.error : new Error(String(end.error));
        for (const sink of remoteSinks.get(ordinaryTabId) ?? []) sink.onStreamError?.(error);
      }
      finalizeRemoteOutputConnection(ordinaryTabId, record, generation, abort, reason);
    },
  });
}

function handleRemoteOutputFrame(
  ordinaryTabId: string,
  record: RemoteOutputRecord,
  generation: number,
  remoteTabId: string,
  message: OutputFrame,
): void {
  if (message.tab_id && message.tab_id !== remoteTabId) return;
  if (message.truncated && typeof message.from === "number") {
    if (!record.recovery.isCurrent(generation)) return;
    const previous = record.recovery.currentCursor();
    const next = { ...previous, offset: planOutputResync(previous?.offset, message.from).offset };
    record.recovery.noteProgress(generation, next);
    for (const target of remoteSinks.get(ordinaryTabId) ?? []) {
      deliverTruncation(ordinaryTabId, target, message.from, next.boot ?? "");
    }
    return;
  }
  if (typeof message.output === "string") {
    const prev = record.recovery.currentCursor();
    const next: RemoteOutputCursor = {
      boot: message.boot ?? prev?.boot,
      ptyId: message.pty_id ?? prev?.ptyId,
      offset: typeof message.offset === "number" ? message.offset : prev?.offset ?? 0,
    };
    if (!record.recovery.noteProgress(generation, next)) return; // duplicate replay or stale generation dropped.
    for (const target of remoteSinks.get(ordinaryTabId) ?? []) deliverOutput(ordinaryTabId, target, { ...message, boot: next.boot });
    return;
  }
  if (message.boot) {
    const previous = record.recovery.currentCursor();
    const offset = previous?.boot === message.boot ? previous.offset : 0;
    record.recovery.noteProgress(generation, { boot: message.boot, offset, ptyId: previous?.boot === message.boot ? previous.ptyId : undefined });
    return;
  }
  if (!record.recovery.isCurrent(generation)) return; // stale generation never delivers control frames.
  for (const target of remoteSinks.get(ordinaryTabId) ?? []) {
    if (message.ended) target.onEnded();
  }
}

function finalizeRemoteOutputConnection(
  ordinaryTabId: string,
  record: RemoteOutputRecord,
  generation: number,
  abort: AbortController,
  reason: StreamEndReason,
): void {
  const decision = record.recovery.handleStreamEnd(generation, reason);
  resolveRemoteSettlement(record, generation);
  if (decision === "stale") return; // superseded/disposed: the current generation owns controller + state.
  record.controller = null;
  if (abort.signal.aborted) return; // unsubscribe/forced abort: the caller drives the next connection.
  if (!remoteSinks.has(ordinaryTabId)) return;
  if (decision === "reconnect") openRemoteOutputConnection(ordinaryTabId, record);
  // decision === "wait": stay dark until resumeRemoteOutputStreams() signals a resync.
}

function resolveRemoteSettlement(record: RemoteOutputRecord, generation: number): void {
  const settled = record.settlements.get(generation);
  record.settlements.delete(generation);
  settled?.resolve();
}

onRemoteTransportCredentials(() => resumeRemoteOutputStreams());

export function resumeRemoteOutputStreams(): void {
  for (const [ordinaryTabId, record] of remoteRecords) {
    if (!remoteSinks.has(ordinaryTabId)) continue;
    // Never supersede a connection still awaiting its first frame — that dropped the initial replay and blanked the remote terminal.
    if (record.recovery.currentCursor() == null && !record.recovery.isAwaitingSignal()) continue;
    if (record.recovery.signal() === "stale") continue;
    record.controller?.abort();
    openRemoteOutputConnection(ordinaryTabId, record);
  }
}

export function remoteOutputSubscriptionGeneration(ordinaryTabId: string): number {
  return remoteRecords.get(ordinaryTabId)?.recovery.generation() ?? 0;
}

export function remoteOutputSubscriptionSnapshot(ordinaryTabId: string): { generation: number; hasController: boolean } {
  const record = remoteRecords.get(ordinaryTabId);
  return { generation: record?.recovery.generation() ?? 0, hasController: record?.controller != null };
}

export function waitForRemoteOutputGenerationSettlement(ordinaryTabId: string, generation: number): Promise<void> {
  return remoteRecords.get(ordinaryTabId)?.settlements.get(generation)?.promise ?? Promise.resolve();
}

export function pokeOutputStream(): void {
  if (!started) return;
  recoverOutputStream();
}

onResync(() => pokeOutputStream());

/** Tear down (peer switch) — next subscription reconnects to the current host. */
export function resetOutputStream(): void {
  started = false;
  aggregate.dispose();
  window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, recoverOutputStream);
  streamAbort = null;
  retainedOutput.clear();
  offsets.clear();
  rings.clear();
  if (sinks.size > 0) ensureStarted();
}
