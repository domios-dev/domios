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
import { planOutputReplay, TerminalOutputReplay, type OutputCursor } from "./outputReplay";
import { planOutputResync } from "./outputResync";
import { DEV_BUILD } from "./devBuild";
import { acceptsTerminalOutput } from "./terminalFrameGate";
import { verifyStreamMuxes } from "./streamMux";

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
  onOutput(text: string): void | Promise<void>;
  onRendered?(offset: number): void;
  onDisconnected?(): void;
  onReset?(): void;
  onStarting?(): void;
  onSubscribed?(): void;
  onReady?(): void;
  onReplayComplete?(): void | Promise<void>;
  onBytes?(bytes: Uint8Array): void | Promise<void>;
  onTruncated(from: number): void;
  onEnded(): void;
  onStreamError?(error: Error): void;
}

interface OutputFrame {
  tab_id?: string;
  b64?: string;
  pty_id?: string;
  offset?: number;
  truncated?: boolean;
  reset?: boolean;
  from?: number;
  ended?: boolean;
  boot_changed?: boolean;
  boot?: string;
  replay_complete?: boolean;
}

const BOOT_KEY = "codeterm-web-output-boot";
const replay = new TerminalOutputReplay();
const retainedOutput = new OutputRetention();
const aggregate = createAggregateOutputRegistry(openAggregateStream);
const sinkIdentities = new WeakMap<TabOutputSink, { terminal: object; ptyId: string; boundPtyId?: string }>();

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
const completedReplays = new Map<string, OutputFrame>();
interface SinkSubscription {
  readonly cancelled: boolean;
  onCancel(listener: () => void): () => void;
  cancel(): void;
}

function createSinkSubscription(): SinkSubscription {
  let cancelled = false;
  const listeners = new Set<() => void>();
  return {
    get cancelled() { return cancelled; },
    onCancel(listener) {
      if (cancelled) {
        listener();
        return () => {};
      }
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    cancel() {
      if (cancelled) return;
      cancelled = true;
      for (const listener of listeners) listener();
      listeners.clear();
    },
  };
}

const sinkSubscriptions = new WeakMap<TabOutputSink, SinkSubscription>();
const renderedCursors = new WeakMap<object, Map<string, OutputCursor>>();

interface AggregateRenderJob {
  kind: "output" | "replay_complete" | "truncated" | "catchup" | "ended";
  tabId: string;
  boot: string;
  ptyId: string;
  frame: OutputFrame;
  bytes?: Uint8Array;
  size: number;
  targetSink?: TabOutputSink;
  onComplete?: () => void;
}

interface AggregateRenderQueue {
  key: string;
  tabId: string;
  boot: string;
  ptyId: string;
  outputGeneration: number;
  jobs: AggregateRenderJob[];
  cursors: Map<object, OutputCursor>;
  pendingBytes: number;
  pendingFrames: number;
  active: boolean;
  running: boolean;
  overflowFrame?: OutputFrame;
  overflowCompletions?: Array<() => void>;
  overflowTruncation?: OutputFrame;
  overflowReplayComplete?: { frame: OutputFrame; targetSink?: TabOutputSink };
  overflowEnded?: OutputFrame;
}

const aggregateRenderQueues = new Map<string, AggregateRenderQueue>();
const MAX_AGGREGATE_RENDER_BYTES = 256 * 1024;
const MAX_AGGREGATE_RENDER_FRAMES = 16;

let boot: string | null = readBoot();
let streamAbort: AbortController | null = null;
let started = false;
let aggregateStarting = false;
let outputGeneration = 0;

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
  const tabs: Record<string, { offset: number; pty_id: string | null }> = {};
  for (const tab of sinks.keys()) {
    const bound = [...(sinks.get(tab) ?? [])].map(sink => sinkIdentities.get(sink)?.boundPtyId).find(Boolean);
    tabs[tab] = { offset: offsets.get(tab) ?? 0, pty_id: rings.get(tab)?.ptyId ?? bound ?? null };
  }
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

function accountOutputFrame(bytes?: Uint8Array) {
  const perf = window.__outputStreamPerf;
  if (perf) {
    perf.frames += 1;
    perf.lastFrameAt = performance.now();
    perf.bytes += bytes?.length ?? 0;
  }
  return perf;
}

function aggregateRenderKey(tabId: string, bootId: string, ptyId: string): string {
  return JSON.stringify([tabId, bootId, ptyId]);
}

function renderedCursor(terminal: object, tabId: string, bootId: string, ptyId: string): OutputCursor | undefined {
  const cursor = renderedCursors.get(terminal)?.get(tabId);
  return cursor?.boot === bootId && cursor.ptyId === ptyId ? cursor : undefined;
}

function saveRenderedCursor(terminal: object, tabId: string, cursor: OutputCursor): void {
  let tabs = renderedCursors.get(terminal);
  if (!tabs) {
    tabs = new Map();
    renderedCursors.set(terminal, tabs);
  }
  tabs.set(tabId, cursor);
}

function isCurrentAggregateRenderQueue(queue: AggregateRenderQueue): boolean {
  return queue.active && queue.outputGeneration === outputGeneration && aggregateRenderQueues.get(queue.key) === queue;
}

function invalidateAggregateRenderQueue(queue: AggregateRenderQueue): void {
  queue.active = false;
  queue.jobs = [];
  queue.pendingBytes = 0;
  queue.pendingFrames = 0;
  queue.overflowFrame = undefined;
  queue.overflowCompletions = undefined;
  queue.overflowTruncation = undefined;
  queue.overflowReplayComplete = undefined;
  queue.overflowEnded = undefined;
  if (aggregateRenderQueues.get(queue.key) === queue) aggregateRenderQueues.delete(queue.key);
}

function invalidateAggregateRenderQueues(tabId?: string): void {
  for (const queue of aggregateRenderQueues.values()) {
    if (tabId === undefined || queue.tabId === tabId) invalidateAggregateRenderQueue(queue);
  }
}

function aggregateRenderQueue(tabId: string, bootId: string, ptyId: string): AggregateRenderQueue {
  const key = aggregateRenderKey(tabId, bootId, ptyId);
  for (const queue of aggregateRenderQueues.values()) {
    if (queue.tabId === tabId && queue.key !== key) invalidateAggregateRenderQueue(queue);
  }
  let queue = aggregateRenderQueues.get(key);
  if (!queue) {
    queue = {
      key, tabId, boot: bootId, ptyId, outputGeneration,
      jobs: [], cursors: new Map(), pendingBytes: 0, pendingFrames: 0,
      active: true, running: false,
    };
    aggregateRenderQueues.set(key, queue);
  }
  return queue;
}

function aggregateRenderHasCapacity(queue: AggregateRenderQueue, job: AggregateRenderJob): boolean {
  return queue.pendingFrames < MAX_AGGREGATE_RENDER_FRAMES
    && (queue.pendingFrames === 0 || queue.pendingBytes + job.size <= MAX_AGGREGATE_RENDER_BYTES);
}

function hasAggregateRenderOverflow(queue: AggregateRenderQueue): boolean {
  return queue.overflowFrame !== undefined || queue.overflowTruncation !== undefined
    || queue.overflowReplayComplete !== undefined || queue.overflowEnded !== undefined;
}

function enqueueAggregateRender(
  tabId: string,
  frame: OutputFrame,
  kind: AggregateRenderJob["kind"],
  bytes?: Uint8Array,
  options?: { targetSink?: TabOutputSink; onComplete?: () => void },
): void {
  const ptyId = frame.pty_id ?? rings.get(tabId)?.ptyId
    ?? [...(sinks.get(tabId) ?? [])].map(sink => sinkIdentities.get(sink)?.boundPtyId).find(Boolean)
    ?? tabId;
  const queue = aggregateRenderQueue(tabId, frame.boot ?? boot ?? "", ptyId);
  const job: AggregateRenderJob = {
    kind, tabId, boot: frame.boot ?? boot ?? "", ptyId,
    frame: kind === "output" || kind === "catchup"
      ? { tab_id: tabId, pty_id: ptyId, boot: frame.boot ?? boot ?? "", from: frame.from, offset: frame.offset }
      : frame,
    bytes,
    size: Math.max(1, bytes?.length ?? 0),
    targetSink: options?.targetSink,
    onComplete: options?.onComplete,
  };
  if (!isCurrentAggregateRenderQueue(queue)) return;
  const full = !aggregateRenderHasCapacity(queue, job);
  if (kind === "output" || kind === "catchup") {
    if (hasAggregateRenderOverflow(queue) || full) {
      // OutputRetention owns the bounded bytes; the queue keeps one coalesced catch-up cursor.
      queue.overflowFrame = {
        tab_id: tabId, pty_id: ptyId, boot: queue.boot, offset: frame.offset,
      };
      if (options?.onComplete) {
        queue.overflowCompletions ??= [];
        queue.overflowCompletions.push(options.onComplete);
      }
    } else {
      queue.pendingBytes += job.size;
      queue.pendingFrames++;
      queue.jobs.push(job);
    }
  } else if (kind === "truncated" && (hasAggregateRenderOverflow(queue) || full)) {
    queue.overflowTruncation = frame;
  } else if (kind === "replay_complete" && (hasAggregateRenderOverflow(queue) || full)) {
    queue.overflowReplayComplete = queue.overflowReplayComplete
      ? {
        frame,
        targetSink: queue.overflowReplayComplete.targetSink === options?.targetSink ? options?.targetSink : undefined,
      }
      : { frame, targetSink: options?.targetSink };
  } else if (kind === "ended" && (hasAggregateRenderOverflow(queue) || full)) {
    queue.overflowEnded = frame;
  } else if (!full) {
    queue.pendingBytes += job.size;
    queue.pendingFrames++;
    queue.jobs.push(job);
  }
  drainAggregateRenderQueue(queue);
}

function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
  return value !== null && (typeof value === "object" || typeof value === "function") && "then" in value;
}

function activeAggregateSink(queue: AggregateRenderQueue, sink: TabOutputSink, subscription: SinkSubscription | undefined): boolean {
  return isCurrentAggregateRenderQueue(queue) && subscription !== undefined && !subscription.cancelled
    && sinkSubscriptions.get(sink) === subscription && sinks.get(queue.tabId)?.has(sink) === true;
}

function rollbackQueueCursor(
  queue: AggregateRenderQueue,
  terminal: object,
  expected: OutputCursor,
  previous: OutputCursor | undefined,
): void {
  if (!isCurrentAggregateRenderQueue(queue) || queue.cursors.get(terminal) !== expected) return;
  if (previous) queue.cursors.set(terminal, previous);
  else queue.cursors.delete(terminal);
}

function awaitSinkCompletion(
  subscription: SinkSubscription,
  completion: PromiseLike<unknown>,
  finish: () => void,
  isActive: () => boolean,
  onReject?: () => void,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    let settled = false;
    let removeCancel = () => {};
    const done = () => {
      if (settled) return;
      settled = true;
      removeCancel();
      finish();
      resolve();
    };
    const fail = (error: unknown) => {
      if (settled) return;
      settled = true;
      removeCancel();
      onReject?.();
      if (isActive()) reject(error);
      else resolve();
    };
    removeCancel = subscription.onCancel(done);
    if (!settled) completion.then(done, fail);
  });
}

function acceptsAggregateJob(job: AggregateRenderJob, sink: TabOutputSink): boolean {
  if (job.targetSink && job.targetSink !== sink) return false;
  if (job.kind === "ended" || (job.kind === "truncated" && !job.frame.reset)) {
    if (job.frame.pty_id === undefined) {
      const frameTabId = parseCompositeTabId(job.tabId)?.remoteTabId ?? job.tabId;
      return job.frame.tab_id === undefined || frameTabId === job.frame.tab_id;
    }
  }
  return acceptsSinkOutput(job.tabId, sink, job.frame);
}

function deliverAggregateOutput(queue: AggregateRenderQueue, sink: TabOutputSink, job: AggregateRenderJob): void | Promise<void> {
  if (!job.bytes || !acceptsSinkOutput(job.tabId, sink, job.frame)) return;
  const subscription = sinkSubscriptions.get(sink);
  if (!subscription) return;
  const identity = sinkIdentities.get(sink) ?? { terminal: sink, ptyId: queue.ptyId };
  const previous = queue.cursors.get(identity.terminal)
    ?? renderedCursor(identity.terminal, job.tabId, queue.boot, queue.ptyId);
  const result = planOutputReplay(previous, {
    boot: queue.boot, ptyId: queue.ptyId,
    offset: job.frame.offset ?? job.bytes.length, bytes: job.bytes,
  });
  if (result.bytes.length === 0) return;
  queue.cursors.set(identity.terminal, result.cursor);
  let completion: void | Promise<void>;
  try {
    completion = writeSinkBytes(sink, result.bytes);
  } catch (error) {
    rollbackQueueCursor(queue, identity.terminal, result.cursor, previous);
    throw error;
  }
  const finish = () => {
    if (!activeAggregateSink(queue, sink, subscription)) {
      rollbackQueueCursor(queue, identity.terminal, result.cursor, previous);
      return;
    }
    saveRenderedCursor(identity.terminal, job.tabId, result.cursor);
    if (typeof job.frame.offset === "number") sink.onRendered?.(job.frame.offset);
  };
  if (!isPromiseLike(completion)) {
    finish();
    return;
  }
  return awaitSinkCompletion(
    subscription,
    completion,
    finish,
    () => activeAggregateSink(queue, sink, subscription),
    () => rollbackQueueCursor(queue, identity.terminal, result.cursor, previous),
  );
}

function deliverAggregateReplayComplete(queue: AggregateRenderQueue, sink: TabOutputSink, job: AggregateRenderJob): void | Promise<void> {
  if (!acceptsSinkOutput(job.tabId, sink, job.frame)) return;
  const subscription = sinkSubscriptions.get(sink);
  if (!subscription) return;
  const identity = sinkIdentities.get(sink) ?? { terminal: sink, ptyId: queue.ptyId };
  const completion = sink.onReplayComplete?.();
  const finish = () => {
    if (!activeAggregateSink(queue, sink, subscription) || typeof job.frame.offset !== "number") return;
    const previous = queue.cursors.get(identity.terminal)
      ?? renderedCursor(identity.terminal, job.tabId, queue.boot, queue.ptyId);
    const cursor = {
      boot: queue.boot, ptyId: queue.ptyId,
      offset: Math.max(previous?.offset ?? 0, job.frame.offset),
    };
    queue.cursors.set(identity.terminal, cursor);
    saveRenderedCursor(identity.terminal, job.tabId, cursor);
    sink.onRendered?.(cursor.offset);
  };
  if (!isPromiseLike(completion)) {
    finish();
    return;
  }
  return awaitSinkCompletion(
    subscription,
    completion,
    finish,
    () => activeAggregateSink(queue, sink, subscription),
  );
}

function deliverAggregateTruncation(queue: AggregateRenderQueue, sink: TabOutputSink, job: AggregateRenderJob): void {
  if (typeof job.frame.from !== "number" || !acceptsAggregateJob(job, sink)) return;
  const subscription = sinkSubscriptions.get(sink);
  if (!subscription) return;
  const identity = sinkIdentities.get(sink) ?? { terminal: sink, ptyId: queue.ptyId };
  const previous = queue.cursors.get(identity.terminal)
    ?? renderedCursor(identity.terminal, job.tabId, queue.boot, queue.ptyId);
  const result = job.frame.reset
    ? { offset: job.frame.from, reportGap: true }
    : planOutputResync(previous?.offset, job.frame.from);
  if (!result.reportGap) return;
  const cursor = { boot: queue.boot, ptyId: queue.ptyId, offset: result.offset };
  queue.cursors.set(identity.terminal, cursor);
  try {
    sink.onTruncated(job.frame.from);
  } catch (error) {
    rollbackQueueCursor(queue, identity.terminal, cursor, previous);
    throw error;
  }
  if (activeAggregateSink(queue, sink, subscription)) {
    saveRenderedCursor(identity.terminal, job.tabId, cursor);
  } else {
    rollbackQueueCursor(queue, identity.terminal, cursor, previous);
  }
}

function deliverAggregateRenderJob(queue: AggregateRenderQueue, job: AggregateRenderJob): void | Promise<void> {
  const targets = [...(sinks.get(job.tabId) ?? [])].filter(sink => acceptsAggregateJob(job, sink));
  let pendingPromises: Promise<void>[] | undefined;
  try {
    for (const sink of targets) {
      if (!isCurrentAggregateRenderQueue(queue) || !sinks.get(job.tabId)?.has(sink)) continue;
      let result: void | Promise<void> = undefined;
      if (job.kind === "output") result = deliverAggregateOutput(queue, sink, job);
      else if (job.kind === "replay_complete") result = deliverAggregateReplayComplete(queue, sink, job);
      else if (job.kind === "truncated") deliverAggregateTruncation(queue, sink, job);
      else if (job.kind === "catchup") {
        deliverAggregateTruncation(queue, sink, { ...job, kind: "truncated" });
        result = deliverAggregateOutput(queue, sink, { ...job, kind: "output" });
      } else {
        sink.onEnded();
      }
      if (isPromiseLike(result)) {
        if (!pendingPromises) pendingPromises = [];
        pendingPromises.push(result);
      }
    }
  } catch (error) {
    if (pendingPromises) for (const promise of pendingPromises) void promise.catch(() => {});
    throw error;
  }
  if (pendingPromises) return Promise.all(pendingPromises).then(() => undefined);
}

function reportAggregateRenderFailure(queue: AggregateRenderQueue, error: unknown): void {
  const failure = error instanceof Error ? error : new Error(String(error));
  for (const sink of sinks.get(queue.tabId) ?? []) {
    try { sink.onStreamError?.(failure); } catch {}
  }
}

function takeDeferredAggregateRenderJob(queue: AggregateRenderQueue): AggregateRenderJob | undefined {
  if (queue.overflowTruncation) {
    const frame = queue.overflowTruncation;
    queue.overflowTruncation = undefined;
    return { kind: "truncated", tabId: queue.tabId, boot: queue.boot, ptyId: queue.ptyId, frame, size: 1 };
  }
  if (queue.overflowFrame) {
    queue.overflowFrame = undefined;
    const completions = queue.overflowCompletions;
    queue.overflowCompletions = undefined;
    const cached = retainedOutput.snapshot(queue.tabId);
    if (cached && cached.boot === queue.boot && cached.ptyId === queue.ptyId) {
      const frame = {
        tab_id: queue.tabId, pty_id: queue.ptyId, boot: queue.boot,
        from: cached.offset - cached.bytes.length, offset: cached.offset,
      };
      return {
        kind: "catchup", tabId: queue.tabId, boot: queue.boot, ptyId: queue.ptyId,
        frame, bytes: cached.bytes, size: Math.max(1, cached.bytes.length),
        onComplete: completions ? () => { for (const done of completions) done(); } : undefined,
      };
    }
  }
  if (queue.overflowReplayComplete) {
    const { frame, targetSink } = queue.overflowReplayComplete;
    queue.overflowReplayComplete = undefined;
    return { kind: "replay_complete", tabId: queue.tabId, boot: queue.boot, ptyId: queue.ptyId, frame, size: 1, targetSink };
  }
  if (queue.overflowEnded) {
    const frame = queue.overflowEnded;
    queue.overflowEnded = undefined;
    return { kind: "ended", tabId: queue.tabId, boot: queue.boot, ptyId: queue.ptyId, frame, size: 1 };
  }
  return undefined;
}

function drainAggregateRenderQueue(queue: AggregateRenderQueue): void {
  if (queue.running || !isCurrentAggregateRenderQueue(queue)) return;
  queue.running = true;
  const finishJob = (job: AggregateRenderJob) => {
    if (isCurrentAggregateRenderQueue(queue)) {
      queue.pendingBytes -= job.size;
      queue.pendingFrames--;
    }
  };
  const failJob = (error: unknown) => {
    if (isCurrentAggregateRenderQueue(queue)) reportAggregateRenderFailure(queue, error);
    invalidateAggregateRenderQueue(queue);
  };
  const runOnComplete = (job: AggregateRenderJob) => {
    if (!isCurrentAggregateRenderQueue(queue) || !job.onComplete) return;
    try {
      job.onComplete();
    } catch (error) {
      failJob(error);
    }
  };
  const step = (): void => {
    while (isCurrentAggregateRenderQueue(queue) && (queue.jobs.length > 0 || hasAggregateRenderOverflow(queue))) {
      const deferred = queue.jobs.length === 0;
      const job = deferred ? takeDeferredAggregateRenderJob(queue) : queue.jobs.shift();
      if (!job) continue;
      if (deferred) {
        queue.pendingBytes += job.size;
        queue.pendingFrames++;
      }
      let result: void | Promise<void> = undefined;
      try {
        result = deliverAggregateRenderJob(queue, job);
      } catch (error) {
        failJob(error);
        finishJob(job);
        continue;
      }
      if (isPromiseLike(result)) {
        result.then(
          () => {
            runOnComplete(job);
            finishJob(job);
            step();
          },
          (error) => {
            failJob(error);
            finishJob(job);
            step();
          },
        );
        return;
      }
      runOnComplete(job);
      finishJob(job);
    }
    queue.running = false;
    if (isCurrentAggregateRenderQueue(queue) && queue.jobs.length === 0 && !hasAggregateRenderOverflow(queue)) {
      aggregateRenderQueues.delete(queue.key);
    }
  };
  step();
}

function handleFrame(msg: OutputFrame): void {
  const bytes = typeof msg.b64 === "string" ? decodeOutputBytes(msg.b64) : undefined;
  const perf = accountOutputFrame(bytes);
  if (msg.boot_changed && msg.boot) {
    outputGeneration++;
    invalidateAggregateRenderQueues();
    for (const set of sinks.values()) for (const sink of set) sink.onReset?.();
    // boot-scoped offsets and reconnect — the boot-matched cursor full-replays.
    saveBoot(msg.boot);
    offsets.clear();
    rings.clear();
    completedReplays.clear();
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
  if (msg.replay_complete) {
    if (set && ![...set].some(sink => acceptsSinkOutput(tab, sink, msg))) return;
    completedReplays.set(tab, msg);
    if (set) enqueueAggregateRender(tab, msg, "replay_complete");
    return;
  }
  if (bytes && set && ![...set].some(sink => acceptsSinkOutput(tab, sink, msg))) return;
  if (typeof msg.offset === "number" && bytes) {
    const framePtyId = msg.pty_id
      ?? [...(set ?? [])].map(sink => sinkIdentities.get(sink)?.boundPtyId).find(Boolean)
      ?? tab;
    const ring = { boot: msg.boot ?? boot ?? "", ptyId: framePtyId };
    const previous = rings.get(tab);
    const sameRing = previous?.boot === ring.boot && previous.ptyId === ring.ptyId;
    if (!sameRing) invalidateAggregateRenderQueues(tab);
    offsets.set(tab, sameRing ? Math.max(offsets.get(tab) ?? 0, msg.offset) : msg.offset);
    rings.set(tab, ring);
    retainedOutput.append(tab, {
      boot: ring.boot, ptyId: ring.ptyId, offset: msg.offset,
      bytes,
    });
    if (set) {
      perf && (perf.batches += 1);
      enqueueAggregateRender(tab, msg, "output", bytes);
    }
    return;
  }
  if (msg.truncated && typeof msg.from === "number") {
    if (msg.reset && set && ![...set].some(sink => acceptsSinkOutput(tab, sink, msg))) return;
    offsets.set(tab, msg.reset ? msg.from : planOutputResync(offsets.get(tab), msg.from).offset);
    if (msg.reset) {
      invalidateAggregateRenderQueues(tab);
      rings.set(tab, { boot: msg.boot ?? boot ?? "", ptyId: msg.pty_id });
      retainedOutput.delete(tab); completedReplays.delete(tab);
    }
    if (set) enqueueAggregateRender(tab, msg, "truncated");
    return;
  }
  if (msg.ended) {
    if (set) enqueueAggregateRender(tab, msg, "ended");
  }
}

function deliverTruncation(tabId: string, sink: TabOutputSink, from: number, bootId: string, reset = false): void {
  const identity = sinkIdentities.get(sink) ?? { terminal: sink, ptyId: tabId };
  replay.resync(identity.terminal, tabId, bootId, identity.ptyId, from, () => sink.onTruncated(from), reset);
}

function decodeOutputBytes(b64: string): Uint8Array {
  const binary = atob(b64.replace(/-/g, "+").replace(/_/g, "/"));
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

function acceptsSinkOutput(tabId: string, sink: TabOutputSink, frame: OutputFrame): boolean {
  const frameTabId = parseCompositeTabId(tabId)?.remoteTabId ?? tabId;
  return acceptsTerminalOutput(frameTabId, sinkIdentities.get(sink)?.boundPtyId, frame.tab_id, frame.pty_id);
}

function deliverReplayComplete(tabId: string, sink: TabOutputSink, frame: OutputFrame): void | Promise<void> {
  if (!acceptsSinkOutput(tabId, sink, frame)) return;
  const generation = outputGeneration;
  return Promise.resolve(sink.onReplayComplete?.()).then(() => {
    if (generation === outputGeneration && typeof frame.offset === "number") sink.onRendered?.(frame.offset);
  });
}

function deliverOutput(tabId: string, sink: TabOutputSink, frame: OutputFrame, bytes: Uint8Array): void | Promise<void> {
  if (!acceptsSinkOutput(tabId, sink, frame)) return;
  const identity = sinkIdentities.get(sink) ?? { terminal: sink, ptyId: frame.pty_id ?? tabId };
  const generation = outputGeneration;
  return replay.deliver(identity.terminal, tabId, {
    boot: frame.boot ?? boot ?? "", ptyId: frame.pty_id ?? identity.ptyId,
    offset: frame.offset ?? bytes.length, bytes,
  }, (payload) => Promise.resolve(writeSinkBytes(sink, payload)).then(() => {
    if (generation === outputGeneration && typeof frame.offset === "number") sink.onRendered?.(frame.offset);
  }));
}

function writeSinkBytes(sink: TabOutputSink, bytes: Uint8Array): void | Promise<void> {
  if (sink.onBytes) return sink.onBytes(bytes);
  return sink.onOutput(new TextDecoder().decode(bytes));
}

let capacityRetry: ReturnType<typeof setTimeout> | undefined;
let capacityResumes = 0;
const CAPACITY_RESUME_DELAYS = [125, 250];

function clearCapacityRetry(): void {
  clearTimeout(capacityRetry); capacityRetry = undefined; capacityResumes = 0;
}

function recoverOutputStream(): void {
  clearCapacityRetry();
  aggregate.recover();
}

function openAggregateStream(): () => void {
  clearTimeout(capacityRetry); capacityRetry = undefined;
  const abort = new AbortController();
  streamAbort = abort;
  aggregateStarting = false;
  let progressed = false;
  const client = outputClient ?? createTransportClient({ origin: apiHost(), token: currentToken });
  client.subscribe(outputStreamUrl().slice(client.origin().length), (_name, data) => {
    if (abort.signal.aborted || streamAbort !== abort) return;
    const frame = data as OutputFrame;
    if (typeof frame?.b64 === "string" || frame?.replay_complete) { progressed = true; capacityResumes = 0; }
    return handleFrame(frame);
  }, {
    signal: abort.signal, raw: true,
    onStarting() {
      if (abort.signal.aborted || streamAbort !== abort) return;
      aggregateStarting = true;
      for (const set of sinks.values()) for (const sink of set) sink.onStarting?.();
    },
    onReady() {
      if (abort.signal.aborted || streamAbort !== abort) return;
      aggregateStarting = false;
      for (const set of sinks.values()) for (const sink of set) sink.onSubscribed?.();
    },
    onActivity() { for (const set of sinks.values()) for (const sink of set) sink.onReady?.(); },
    onEnd(end) {
      if (abort.signal.aborted || streamAbort !== abort) return;
      streamAbort = null;
      aggregateStarting = false;
      aggregate.ended();
      for (const set of sinks.values()) for (const sink of set) sink.onDisconnected?.();
      if (end.reason === "capacity" && (progressed || capacityResumes < CAPACITY_RESUME_DELAYS.length)) {
        if (DEV_BUILD) console.debug("[output-stream] resuming after capacity end", { progressed });
        if (progressed) aggregate.recover();
        else {
          capacityRetry = setTimeout(() => { capacityRetry = undefined; aggregate.recover(); }, CAPACITY_RESUME_DELAYS[capacityResumes++]);
        }
        return;
      }
      if (DEV_BUILD) console.debug("[output-stream] waiting for recovery signal", { reason: end.reason });
      if (end.reason === "abort") return;
      const error = end.error instanceof Error ? end.error
        : new Error(end.reason === "capacity" ? `SSE capacity exceeded (${end.bytes} bytes)` : end.reason === "eof" ? "Output stream ended" : String(end.error));
      for (const set of sinks.values()) for (const sink of set) sink.onStreamError?.(error);
    },
  });
  return () => {
    for (const set of sinks.values()) for (const sink of set) sink.onDisconnected?.();
    abort.abort(); if (streamAbort === abort) streamAbort = null;
  };
}

/** An explicit Retry: replace the aggregate connection (even one that is open but not delivering) from the cursor. */
export function restartOutputStream(): void {
  clearCapacityRetry();
  if (started) aggregate.replace(openAggregateStream);
  else if (sinks.size > 0) ensureStarted();
  resumeRemoteOutputStreams();
}

function ensureStarted(): void {
  if (started || selectedRelayTarget()) return;
  started = true;
  window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, recoverOutputStream);
  aggregate.attach();
}

export function subscribeTabOutput(tabId: string, sink: TabOutputSink, terminal: object = sink, ptyId?: string): () => void {
  sinkIdentities.set(sink, { terminal, ptyId: ptyId ?? tabId, boundPtyId: ptyId });
  sinkSubscriptions.get(sink)?.cancel();
  const subscription = createSinkSubscription();
  sinkSubscriptions.set(sink, subscription);
  const route = tabOutputRoute(tabId, selectedRelayTarget()?.id ?? null);
  if (route) {
    const unsubscribe = subscribeRemoteTabOutput(`mesh:${route.peerId}/${route.tabId}`, route.peerId, sink);
    return () => {
      if (sinkSubscriptions.get(sink) !== subscription) return;
      sinkSubscriptions.delete(sink);
      subscription.cancel();
      unsubscribe();
    };
  }
  let set = sinks.get(tabId);
  if (!set) {
    set = new Set();
    sinks.set(tabId, set);
  }
  set.add(sink);
  if (aggregateStarting) sink.onStarting?.();
  const retained = retainedOutput.snapshot(tabId);
  const cached = retainedOutput.snapshot(tabId, renderedCursor(
    terminal, tabId, retained?.boot ?? boot ?? "", retained?.ptyId ?? ptyId ?? tabId,
  ));
  if (cached && (ptyId === undefined || cached.ptyId === ptyId)) {
    const from = cached.offset - cached.bytes.length;
    const frame = { tab_id: tabId, pty_id: cached.ptyId, boot: cached.boot, from, offset: cached.offset };
    enqueueAggregateRender(tabId, frame, "catchup", cached.bytes, {
      targetSink: sink,
      onComplete: () => {
        if (sinkSubscriptions.get(sink) === subscription && !subscription.cancelled) sink.onReady?.();
      },
    });
  }
  const completed = completedReplays.get(tabId);
  if (completed && acceptsSinkOutput(tabId, sink, completed)) {
    enqueueAggregateRender(tabId, completed, "replay_complete", undefined, { targetSink: sink });
  }
  // Attaching a terminal is an activation: it revives a parked aggregate connection.
  if (started) recoverOutputStream();
  else ensureStarted();
  verifyStreamMuxes();
  return () => {
    if (sinkSubscriptions.get(sink) !== subscription) return;
    sinkSubscriptions.delete(sink);
    subscription.cancel();
    set!.delete(sink);
    if (set!.size === 0) {
      sinks.delete(tabId);
      invalidateAggregateRenderQueues(tabId);
    }
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
  const completed = remoteRecords.get(ordinaryTabId)?.replayComplete;
  if (completed) deliverReplayComplete(ordinaryTabId, sink, completed);
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
  reset?: boolean;
}

interface RemoteOutputRecord {
  peerId: string;
  controller: AbortController | null;
  recovery: RemoteChannelRecovery<RemoteOutputCursor>;
  settlements: Map<number, { promise: Promise<void>; resolve: () => void }>;
  replayComplete?: OutputFrame;
}

const remoteSinks = new Map<string, Set<TabOutputSink>>();
const remoteRecords = new Map<string, RemoteOutputRecord>();

// Strictly-ahead cursor test: a new boot ring or a higher byte offset is forward
// progress; anything else is a frame the server replayed and we already delivered.
function remoteOutputAdvanced(prev: RemoteOutputCursor | null, next: RemoteOutputCursor): boolean {
  if (next.reset) return true;
  if (!prev) return true;
  if (next.boot !== prev.boot || next.ptyId !== prev.ptyId) return true;
  return next.offset > prev.offset;
}

export function remoteOutputSinceCursor(remoteTabId: string, bootId: string, offset: number, ptyId?: string): string {
  const raw = JSON.stringify({ boot: bootId, tabs: { [remoteTabId]: { offset, pty_id: ptyId ?? null } } });
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
  const since = cursor?.boot ? remoteOutputSinceCursor(remoteTabId, cursor.boot, cursor.offset, cursor.ptyId) : undefined;
  const path = remoteTabStreamPath({ peerId: record.peerId, tabId: remoteTabId }, since);
  if (DEV_BUILD) console.debug("[output-stream] remote tab stream", { ordinaryTabId, path, generation });
  const transport = remoteTabTransportHttp();
  const client = outputClient ?? createTransportClient({ origin: transport.base, token: () => remoteTabTransportHttp().token });
  client.subscribe(path, (_name, data) => {
    if (abort.signal.aborted || !record.recovery.isCurrent(generation)) return;
    return handleRemoteOutputFrame(ordinaryTabId, record, generation, remoteTabId, data as OutputFrame);
  }, {
    signal: abort.signal, raw: true, onStarting() {
      deadline.clear();
      if (record.recovery.isCurrent(generation)) for (const sink of remoteSinks.get(ordinaryTabId) ?? []) sink.onStarting?.();
    }, onReady() {
      deadline.arm();
      if (record.recovery.isCurrent(generation)) for (const sink of remoteSinks.get(ordinaryTabId) ?? []) sink.onSubscribed?.();
    }, onActivity() {
      deadline.arm();
      if (record.recovery.isCurrent(generation)) for (const sink of remoteSinks.get(ordinaryTabId) ?? []) sink.onReady?.();
    },
    onEnd(end) {
      deadline.clear();
      const reason: StreamEndReason = end.reason === "http"
        ? streamResponseFailureReason(end.status ?? 500) : end.reason === "eof" ? "eof" : "error";
      if (!abort.signal.aborted && record.recovery.isCurrent(generation) && (end.error || end.reason === "capacity")) {
        const error = end.error instanceof Error ? end.error : new Error(end.reason === "capacity" ? `SSE capacity exceeded (${end.bytes} bytes)` : String(end.error));
        for (const sink of remoteSinks.get(ordinaryTabId) ?? []) sink.onStreamError?.(error);
      }
      finalizeRemoteOutputConnection(ordinaryTabId, record, generation, abort, reason);
    },
  });
}

async function handleRemoteOutputFrame(
  ordinaryTabId: string,
  record: RemoteOutputRecord,
  generation: number,
  remoteTabId: string,
  message: OutputFrame,
): Promise<void> {
  if (message.tab_id && message.tab_id !== remoteTabId) return;
  if (message.replay_complete) {
    const targets = remoteSinks.get(ordinaryTabId);
    if (!targets || ![...targets].some(target => acceptsSinkOutput(ordinaryTabId, target, message))) return;
    record.recovery.noteProgress(generation, {
      boot: message.boot, ptyId: message.pty_id, offset: message.offset ?? 0,
    });
    record.replayComplete = message;
    await Promise.all([...targets].map((target) => deliverReplayComplete(ordinaryTabId, target, message)));
    return;
  }
  if (message.truncated && typeof message.from === "number") {
    if (!record.recovery.isCurrent(generation)) return;
    const previous = record.recovery.currentCursor();
    if (message.reset && ![...(remoteSinks.get(ordinaryTabId) ?? [])].some(target => acceptsSinkOutput(ordinaryTabId, target, message))) return;
    const next = { ...previous, boot: message.boot ?? previous?.boot, ptyId: message.pty_id ?? previous?.ptyId,
      offset: message.reset ? message.from : planOutputResync(previous?.offset, message.from).offset, reset: message.reset };
    if (message.reset) record.replayComplete = undefined;
    record.recovery.noteProgress(generation, next);
    for (const target of remoteSinks.get(ordinaryTabId) ?? []) {
      deliverTruncation(ordinaryTabId, target, message.from, next.boot ?? "", message.reset);
    }
    return;
  }
  if (typeof message.b64 === "string") {
    const targets = remoteSinks.get(ordinaryTabId);
    if (!targets || ![...targets].some(target => acceptsSinkOutput(ordinaryTabId, target, message))) return;
    const bytes = decodeOutputBytes(message.b64);
    const perf = accountOutputFrame(bytes);
    const prev = record.recovery.currentCursor();
    const next: RemoteOutputCursor = {
      boot: message.boot ?? prev?.boot,
      ptyId: message.pty_id ?? prev?.ptyId,
      offset: typeof message.offset === "number" ? message.offset : prev?.offset ?? 0,
    };
    if (!record.recovery.noteProgress(generation, next)) return; // duplicate replay or stale generation dropped.
    if (perf) perf.batches++;
    await Promise.all([...(remoteSinks.get(ordinaryTabId) ?? [])].map((target) => deliverOutput(ordinaryTabId, target, { ...message, boot: next.boot }, bytes)));
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
  outputGeneration++;
  invalidateAggregateRenderQueues();
  clearCapacityRetry();
  completedReplays.clear();
  started = false;
  aggregate.dispose();
  window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, recoverOutputStream);
  streamAbort = null;
  aggregateStarting = false;
  retainedOutput.clear();
  offsets.clear();
  rings.clear();
  if (sinks.size > 0) ensureStarted();
}
