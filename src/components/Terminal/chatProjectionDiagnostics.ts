import { DEV_BUILD } from "../../lib/devBuild";
import type { ChatSessionDiagnosticEvent, ChatSessionDiagnosticSink } from "./useChatSession";

/** Stages of the chat projection pipeline whose tail can diverge. Ordered
 *  causally: a delta produced upstream must reach each stage in turn before the
 *  user sees it. `firstStalledEdge` walks this order and names the earliest gap. */
export type ProjectionStage =
  | "backend_emit"
  | "frontend_authority"
  | "committed_state"
  | "painted_tail";

export const PROJECTION_STAGE_ORDER: readonly ProjectionStage[] = [
  "backend_emit",
  "frontend_authority",
  "committed_state",
  "painted_tail",
];

export interface ProjectionCounters {
  /** Parser/producer output: cumulative messages carried by delivered deltas. */
  producedMessages: number;
  /** Backend emit observed at the frontend: one per delivered delta event. */
  arrived: number;
  /** Frontend arrival that won transcript authority. */
  adopted: number;
  /** Frontend arrival rejected as non-authoritative. */
  dropped: number;
  /** Committed React state (urgent transcript ingestion). */
  committed: number;
  /** Painted/visible tail commits (deferred downstream projection). */
  painted: number;
  /** Tail signatures (`<lastId>#<contentLength>`), not bare ids: streaming grows
   *  the tail message under a stable id, so a content-blind tail would hide a
   *  committed-but-unpainted stream update from `firstStalledEdge`. */
  arrivalTail: string | null;
  authorityTail: string | null;
  committedTail: string | null;
  paintedTail: string | null;
  /** Count of `snapshot_arrival` observations on this live record. Distinguishes
   *  "no reactivation snapshot seen yet" (0) from "observed, and it was empty"
   *  (>0 with a null `snapshotTail`) — the two must not both read as pending. */
  snapshots: number;
  /** Backend recovery coordinates from the LATEST `snapshot_arrival` (re-attach),
   *  set verbatim — an observed-empty snapshot resets `snapshotTail` to null rather
   *  than retaining a stale tail. Not a projection stage: kept off
   *  `firstStalledEdge`; used to attribute what a reactivation recovered to versus
   *  the tab's prior committed tail and coordinates. */
  snapshotTail: string | null;
  snapshotCursor: string | null;
  snapshotOffset: number | null;
}

/** Compact history retained when a tab is forgotten, so a later reactivation can
 *  be causally compared to where the torn-down tab actually left off. This is
 *  archival history, not live tab state: it is never returned by `snapshot`,
 *  never counted by `trackedTabCount`, and never walked by `firstStalledEdge`. */
export interface PriorActivation {
  committedTail: string | null;
  paintedTail: string | null;
  snapshotTail: string | null;
  snapshotCursor: string | null;
  snapshotOffset: number | null;
}

/** How a reactivated snapshot lines up with the forgotten tab's prior committed
 *  tail and backend coordinates. `no_prior`: tab was never torn down. `pending`:
 *  torn down, no reactivation snapshot OBSERVED yet (distinct from observed-empty).
 *  `recovered`: tail id and length ≥ prior AND cursor/offset do not contradict it.
 *  `regressed`: observed-empty after a non-empty prior, OR same id but shorter, OR a
 *  caught-up signature whose backend offset rewound / cursor replays a different
 *  tail. `diverged`: a different tail id (unorderable by signature alone). */
export type ActivationRecovery = "no_prior" | "pending" | "recovered" | "regressed" | "diverged";

export interface StalledEdge {
  from: ProjectionStage;
  to: ProjectionStage;
  /** Tail signature the upstream stage reached. */
  expectedTail: string | null;
  /** Tail signature the downstream stage has actually caught up to. */
  observedTail: string | null;
}

const DEFAULT_MAX_TRACKED_TABS = 64;

function freshCounters(): ProjectionCounters {
  return {
    producedMessages: 0,
    arrived: 0,
    adopted: 0,
    dropped: 0,
    committed: 0,
    painted: 0,
    arrivalTail: null,
    authorityTail: null,
    committedTail: null,
    paintedTail: null,
    snapshots: 0,
    snapshotTail: null,
    snapshotCursor: null,
    snapshotOffset: null,
  };
}

function parseTail(signature: string): { id: string; length: number } | null {
  const hash = signature.lastIndexOf("#");
  if (hash < 0) return null;
  const length = Number(signature.slice(hash + 1));
  if (!Number.isFinite(length)) return null;
  return { id: signature.slice(0, hash), length };
}

/** Signature-gated comparison of a reactivated snapshot tail against the prior
 *  committed tail. Same content signature → recovered; same id but shorter →
 *  regressed; different id → diverged (unorderable by signature alone). */
function classifyRecovery(recovered: string, target: string): ActivationRecovery {
  if (recovered === target) return "recovered";
  const r = parseTail(recovered);
  const t = parseTail(target);
  if (!r || !t) return "diverged";
  if (r.id !== t.id) return "diverged";
  return r.length >= t.length ? "recovered" : "regressed";
}

/** Backend coordinates that contradict a signature that otherwise looks recovered.
 *  Offset is orderable: a re-attach reporting a strictly earlier offset than the
 *  tab's prior attach means the backend rewound. Cursor is opaque, so only its
 *  equality is meaningful: the SAME cursor as the prior attach paired with a
 *  DIFFERENT snapshot tail means the backend replays a different transcript under
 *  an identical position marker. Either is a recovery the tail signature can miss. */
function coordinatesContradictRecovery(prior: PriorActivation, live: ProjectionCounters): boolean {
  if (
    prior.snapshotOffset != null
    && live.snapshotOffset != null
    && live.snapshotOffset < prior.snapshotOffset
  ) return true;
  if (
    prior.snapshotCursor != null
    && live.snapshotCursor != null
    && prior.snapshotCursor === live.snapshotCursor
    && prior.snapshotTail !== live.snapshotTail
  ) return true;
  return false;
}

interface StageTail {
  stage: ProjectionStage;
  tail: string | null;
}

/** Bounded (LRU) collection of monotonic per-tab projection counters plus the
 *  read seam that identifies the first stalled edge. Counts only ever increase;
 *  tails track the newest message id seen at each stage so a legitimately
 *  collapsed paint (many commits → one visible update) is not mistaken for a
 *  stall — only a tail divergence is. */
export class ChatProjectionMonitor {
  private readonly tabs = new Map<string, ProjectionCounters>();
  private readonly priors = new Map<string, PriorActivation>();
  private readonly maxTrackedTabs: number;

  constructor(maxTrackedTabs = DEFAULT_MAX_TRACKED_TABS) {
    this.maxTrackedTabs = Math.max(1, maxTrackedTabs);
  }

  /** A `ChatSessionDiagnosticSink` — pass straight to `useChatSession`. */
  readonly sink: ChatSessionDiagnosticSink = (event) => this.ingest(event);

  ingest(event: ChatSessionDiagnosticEvent): void {
    const counters = this.touch(event.tabId);
    switch (event.edge) {
      case "event_arrival":
        counters.arrived += 1;
        counters.producedMessages += event.messageIds.length;
        counters.arrivalTail = event.tailSignature ?? counters.arrivalTail;
        break;
      case "authority_adopt":
        counters.adopted += 1;
        counters.authorityTail = event.tailSignature ?? counters.authorityTail;
        break;
      case "authority_drop":
        counters.dropped += 1;
        break;
      case "state_commit":
        counters.committed += 1;
        counters.committedTail = event.tailSignature ?? counters.committedTail;
        break;
      case "snapshot_arrival":
        // Latest snapshot wins verbatim: an observed-empty re-attach must reset the
        // tail to null (visible as regression), not silently keep a prior tail.
        counters.snapshots += 1;
        counters.snapshotTail = event.tailSignature;
        counters.snapshotCursor = event.cursor;
        counters.snapshotOffset = event.offset;
        break;
      default:
        break;
    }
  }

  /** Painted tail from the render layer. `tailSignature` is the last visible
   *  message's signature (`<id>#<contentLength>`; null when the visible
   *  projection is empty). Passive observation only — records what was painted,
   *  never influences what renders. */
  recordPaint(tabId: string, tailSignature: string | null): void {
    const counters = this.touch(tabId);
    counters.painted += 1;
    counters.paintedTail = tailSignature;
  }

  /** Drop a tab's LIVE record on unmount so tracking follows component lifecycle
   *  and torn-down tabs do not linger until the LRU cap evicts them. Before
   *  dropping it, archive a compact prior-activation summary (tails + recovery
   *  coordinates) so a later reactivation of the same tab can be causally
   *  compared. The archive is not live state: `snapshot`, `trackedTabCount`, and
   *  `firstStalledEdge` all ignore it. */
  forget(tabId: string): void {
    const counters = this.tabs.get(tabId);
    if (counters) {
      this.priors.delete(tabId);
      this.priors.set(tabId, {
        committedTail: counters.committedTail,
        paintedTail: counters.paintedTail,
        snapshotTail: counters.snapshotTail,
        snapshotCursor: counters.snapshotCursor,
        snapshotOffset: counters.snapshotOffset,
      });
      while (this.priors.size > this.maxTrackedTabs) {
        const oldest = this.priors.keys().next().value;
        if (oldest === undefined) break;
        this.priors.delete(oldest);
      }
    }
    this.tabs.delete(tabId);
  }

  snapshot(tabId: string): ProjectionCounters | null {
    const counters = this.tabs.get(tabId);
    return counters ? { ...counters } : null;
  }

  /** Archived history from the last time this tab was forgotten (deactivated /
   *  switched to Terminal), or null if it was never torn down. Distinct from
   *  `snapshot`, which only sees the live record. */
  priorActivation(tabId: string): PriorActivation | null {
    const prior = this.priors.get(tabId);
    return prior ? { ...prior } : null;
  }

  /** Attribute how the tab's current (reactivated) snapshot recovered relative to
   *  the tail it had committed before it was forgotten. Compares the live
   *  `snapshot_arrival` signature against the archived prior committed tail so an
   *  away→back / Terminal→Chat reconcile is causally observable. */
  activationRecovery(tabId: string): ActivationRecovery {
    const prior = this.priors.get(tabId);
    if (!prior) return "no_prior";
    const live = this.tabs.get(tabId);
    // snapshot NOT observed yet — distinct from an observed-but-empty snapshot.
    if (!live || live.snapshots === 0) return "pending";
    const target = prior.committedTail;
    if (target == null) return "recovered";
    // Observed empty re-attach after a non-empty prior: real content loss, never pending.
    if (live.snapshotTail == null) return "regressed";
    const bySignature = classifyRecovery(live.snapshotTail, target);
    if (bySignature !== "recovered") return bySignature;
    // Signature looks caught up — gate on backend cursor/offset before trusting it.
    return coordinatesContradictRecovery(prior, live) ? "regressed" : "recovered";
  }

  /** First stage boundary where the downstream tail lags the upstream tail, in
   *  causal order. Returns null when the whole chain has caught up (or nothing
   *  has arrived yet). The frozen-tail signature surfaces here as a
   *  `committed_state → painted_tail` edge. */
  firstStalledEdge(tabId: string): StalledEdge | null {
    const counters = this.tabs.get(tabId);
    if (!counters) return null;
    const stages: StageTail[] = [
      { stage: "backend_emit", tail: counters.arrivalTail },
      { stage: "frontend_authority", tail: counters.authorityTail },
      { stage: "committed_state", tail: counters.committedTail },
      { stage: "painted_tail", tail: counters.paintedTail },
    ];
    for (let i = 0; i < stages.length - 1; i += 1) {
      const upstream = stages[i]!;
      const downstream = stages[i + 1]!;
      if (upstream.tail == null) continue;
      if (downstream.tail !== upstream.tail) {
        return {
          from: upstream.stage,
          to: downstream.stage,
          expectedTail: upstream.tail,
          observedTail: downstream.tail,
        };
      }
    }
    return null;
  }

  trackedTabCount(): number {
    return this.tabs.size;
  }

  private touch(tabId: string): ProjectionCounters {
    const existing = this.tabs.get(tabId);
    if (existing) {
      // Move to most-recently-used end of the insertion-ordered map.
      this.tabs.delete(tabId);
      this.tabs.set(tabId, existing);
      return existing;
    }
    const counters = freshCounters();
    this.tabs.set(tabId, counters);
    while (this.tabs.size > this.maxTrackedTabs) {
      const oldest = this.tabs.keys().next().value;
      if (oldest === undefined) break;
      this.tabs.delete(oldest);
    }
    return counters;
  }
}

/** Run `record` after the browser has painted the current commit, not merely
 *  after React commits the DOM. A layout effect fires before paint, so it would
 *  mistime the painted-tail probe; a double rAF lands after the frame the commit
 *  produced is on screen. Returns a canceller (cancels a superseded frame when
 *  renders outrun paints — exactly the starvation the probe measures). Falls back
 *  to a synchronous record when no rAF exists (non-browser/test host). */
export function scheduleAfterPaint(record: () => void): () => void {
  const raf = typeof requestAnimationFrame === "function" ? requestAnimationFrame : null;
  const caf = typeof cancelAnimationFrame === "function" ? cancelAnimationFrame : null;
  if (!raf) {
    record();
    return () => {};
  }
  let inner = 0;
  const outer = raf(() => {
    inner = raf(record);
  });
  return () => {
    caf?.(outer);
    caf?.(inner);
  };
}

/** Process-wide monitor fed by production tabs. Tests construct isolated
 *  `ChatProjectionMonitor` instances instead of sharing this one. */
export const chatProjectionMonitor = new ChatProjectionMonitor();

declare global {
  interface Window {
    __chatProjection?: ChatProjectionMonitor;
  }
}

export interface ChatTraceSelection {
  trace: string;
  tab: string;
  session: string;
  nonce: string;
  epoch: number;
  turn: string;
  item: string;
}
export type ChatTraceArm = Omit<ChatTraceSelection, "turn" | "item">;
type LiveTraceOrigin = { kind?: string; turn?: string };

function validTraceId(id: unknown): id is string {
  return typeof id === "string" && id.length > 0 && id.length <= 128
    && [...id].every(char => char.charCodeAt(0) > 31 && char.charCodeAt(0) !== 127)
    && new TextEncoder().encode(id).length <= 128;
}

function validateTraceSelection(binding: ChatTraceArm, ids: readonly unknown[]): void {
  if (!ids.every(validTraceId) || !/^[A-Za-z0-9-]+$/.test(binding.trace)
    || !Number.isSafeInteger(binding.epoch) || binding.epoch <= 0) throw new Error("Invalid chat trace selection");
}
const traceBinding = ({ trace, tab, session, nonce, epoch }: ChatTraceArm): ChatTraceArm => ({ trace, tab, session, nonce, epoch });

type TraceAuthority = { tab: string; session: string | null; nonce: string | null; epoch?: number; generation?: number };
type TraceRow = { id: string; type?: string; seq?: number; content?: string; toolResult?: string; toolDetail?: string; toolFlowId?: string; toolStatus?: string | null };
interface ChatTraceRecord {
  stage: string;
  clock: "browser-performance-ms";
  at: number;
  unixMs: number;
  binding: string;
  turn: string;
  item: string;
  epoch: number;
  epochVerified: boolean;
  listenerGeneration: number | null;
  sourceSeq: number | null;
  status: string | null;
  contentUnits: number;
  resultUnits: number;
  detailUnits: number;
  observerMs: number;
  mountStart?: number;
  mountEnd?: number;
  mountRevision?: number;
}

function traceFingerprint(parts: readonly string[]): string {
  let hash = 2166136261;
  const encoder = new TextEncoder();
  for (const part of parts) {
    for (const byte of encoder.encode(part)) hash = Math.imul(hash ^ byte, 16777619);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

type TraceMetrics = Pick<ChatTraceRecord, "sourceSeq" | "status" | "contentUnits" | "resultUnits" | "detailUnits">;
const traceMetrics = (row: TraceRow): TraceMetrics => ({ sourceSeq: row.seq ?? null,
  status: ["running", "completed", "failed", "interrupted", "abandoned"].includes(row.toolStatus ?? "") ? row.toolStatus ?? null : null,
  contentUnits: row.content?.length ?? 0, resultUnits: row.toolResult?.length ?? 0, detailUnits: row.toolDetail?.length ?? 0 });
const traceRow = (row: TraceRow): TraceRow => row;
const traceEntry = (entry: { msg: TraceRow }): TraceRow => entry.msg;
const NO_TRACE_WORK = () => {};
type TraceMount = { active: boolean; start: number; end: number; revision: number; isMounted: (id: string, index: number) => boolean };

export class ChatResponsivenessTrace {
  private selection: ChatTraceSelection | null = null;
  private armed: ChatTraceArm | null = null;
  private records: ChatTraceRecord[] = [];
  private dropped = 0;
  private overheadMs = 0;
  private epochVerified = false;
  private generation = 0;
  private cancellations = new Set<() => void>();

  constructor(private readonly clock = () => performance.now(), private readonly wall = Date.now) {}

  select(selection: ChatTraceSelection): void {
    const ids = [selection.trace, selection.tab, selection.session, selection.nonce, selection.turn, selection.item];
    validateTraceSelection(selection, ids);
    this.dispose();
    this.selection = { ...traceBinding(selection), turn: selection.turn, item: selection.item };
    this.records = [];
    this.dropped = 0;
    this.overheadMs = 0;
  }

  arm(binding: ChatTraceArm): void {
    validateTraceSelection(binding, [binding.trace, binding.tab, binding.session, binding.nonce]);
    this.dispose();
    this.armed = traceBinding(binding);
    this.records = [];
    this.dropped = 0;
    this.overheadMs = 0;
  }

  bind(authority: TraceAuthority): void {
    const selection = this.selection ?? this.armed;
    if (!selection || selection.tab !== authority.tab) return;
    if (!this.matches(authority) || selection.epoch !== authority.epoch) { this.dispose(); return; }
    this.epochVerified = true;
  }

  private matches(authority: TraceAuthority): boolean {
    const selection = this.selection ?? this.armed;
    return selection != null && selection.tab === authority.tab && selection.session === authority.session
      && selection.nonce === authority.nonce && (authority.epoch === undefined || selection.epoch === authority.epoch);
  }

  eligible(authority: TraceAuthority): boolean { return this.selection !== null && this.matches(authority); }

  private admit(authority: TraceAuthority, active = true): boolean {
    if (!active || !this.eligible(authority)) return false;
    if (this.records.length >= 2048) { this.dropped++; return false; }
    return true;
  }

  private metrics<T>(rows: readonly T[], readRow: (row: T) => TraceRow, start = 0, end = rows.length) {
    const selection = this.selection;
    if (!selection) return null;
    const last = Math.max(0, Math.min(rows.length, end));
    for (let index = last - 1; index >= Math.max(0, start, last - 1024); index--) {
      const row = readRow(rows[index]);
      if (row.id === selection.item && row.toolFlowId === selection.turn) {
        return { index, metrics: traceMetrics(row) };
      }
    }
    return null;
  }

  private record(stage: string, authority: TraceAuthority, metrics: TraceMetrics, mount?: Pick<TraceMount, "start" | "end" | "revision">): ChatTraceRecord | null {
    if (!this.matches(authority)) return null;
    if (this.records.length >= 2048) { this.dropped++; return null; }
    const selection = this.selection!;
    const record: ChatTraceRecord = {
      stage, clock: "browser-performance-ms", at: this.clock(), unixMs: this.wall(),
      binding: traceFingerprint([selection.tab, selection.session, selection.nonce]),
      turn: traceFingerprint([selection.turn]), item: traceFingerprint([selection.item]), epoch: selection.epoch,
      epochVerified: authority.epoch !== undefined ? authority.epoch === selection.epoch : this.epochVerified,
      listenerGeneration: authority.generation ?? null, ...metrics, observerMs: 0,
      ...(mount ? { mountStart: mount.start, mountEnd: mount.end, mountRevision: mount.revision } : {}),
    };
    this.records.push(record);
    return record;
  }

  private finish(start: number, record: ChatTraceRecord | null = null): void {
    const elapsed = this.clock() - start;
    if (record) record.observerMs = elapsed;
    this.overheadMs += elapsed;
  }

  arrival(authority: TraceAuthority, rows: readonly TraceRow[], origin?: LiveTraceOrigin): void {
    if ((!this.armed && !this.selection) || !this.matches(authority)) return;
    if (this.records.length >= 2048) { this.dropped++; return; }
    const start = this.clock();
    if (authority.epoch === undefined || origin?.kind !== "runtime_live" || !validTraceId(origin.turn)) {
      this.finish(start); return;
    }
    let metrics: TraceMetrics | null = null;
    if (this.armed) {
      for (let index = 0; index < Math.min(rows.length, 1024); index++) {
        const row = rows[index];
        if (row.type !== "assistant" || row.toolFlowId !== origin.turn || !validTraceId(row.id)) continue;
        this.selection = { ...this.armed, turn: origin.turn, item: row.id };
        this.armed = null;
        this.epochVerified = true;
        metrics = traceMetrics(row);
        break;
      }
      if (this.armed && rows.length > 1024) this.dropped++;
    } else if (this.selection?.turn === origin.turn) metrics = this.metrics(rows, traceRow)?.metrics ?? null;
    const record = metrics ? this.record("frontend_arrival", authority, metrics) : null;
    this.finish(start, record);
  }

  observe(stage: string, authority: TraceAuthority, rows: readonly TraceRow[]): void {
    if (!this.admit(authority)) return;
    const start = this.clock();
    const found = this.metrics(rows, traceRow);
    const record = found ? this.record(stage, authority, found.metrics) : null;
    this.finish(start, record);
  }

  rendered(authority: TraceAuthority, entries: readonly { msg: TraceRow }[], mount: TraceMount): () => void {
    if (!this.admit(authority, mount.active)) return NO_TRACE_WORK;
    const start = this.clock();
    const found = this.metrics(entries, traceEntry, mount.start, mount.end);
    if (!found || !this.selection || !mount.isMounted(this.selection.item, found.index)) { this.finish(start); return NO_TRACE_WORK; }
    const item = this.selection.item;
    const record = this.record("rendered_commit", authority, found.metrics, mount);
    const cancel = this.scheduleMetrics(authority, found.metrics, () => mount.isMounted(item, found.index), mount);
    this.finish(start, record);
    return cancel;
  }

  afterPaint(authority: TraceAuthority, rows: readonly TraceRow[], isMounted: () => boolean): () => void {
    if (!this.admit(authority)) return NO_TRACE_WORK;
    const start = this.clock();
    const found = this.metrics(rows, traceRow);
    const cancel = found && isMounted() ? this.scheduleMetrics(authority, found.metrics, isMounted) : NO_TRACE_WORK;
    this.finish(start);
    return cancel;
  }
  private scheduleMetrics(authority: TraceAuthority, metrics: TraceMetrics, isMounted: () => boolean, mount?: Pick<TraceMount, "start" | "end" | "revision">): () => void {
    const generation = this.generation;
    let completed = false;
    let stop = NO_TRACE_WORK;
    const cancel = scheduleAfterPaint(() => {
      const start = this.clock();
      completed = true;
      this.cancellations.delete(stop);
      const record = generation === this.generation && this.admit(authority) && isMounted()
        ? this.record("paint_opportunity", authority, metrics, mount) : null;
      this.finish(start, record);
    });
    stop = () => {
      const start = this.clock();
      cancel();
      this.cancellations.delete(stop);
      this.finish(start);
    };
    if (!completed) this.cancellations.add(stop);
    return stop;
  }
  snapshot() {
    return { trace: this.selection?.trace ?? this.armed?.trace ?? null, timeOrigin: performance.timeOrigin,
      selection: this.selection ? { ...this.selection } : null, armed: this.armed ? { ...this.armed } : null,
      records: this.records.map(record => ({ ...record })), dropped: this.dropped, observerMs: this.overheadMs,
      active: this.selection !== null || this.armed !== null };
  }

  dispose(): void {
    for (const cancel of this.cancellations) cancel();
    this.cancellations.clear();
    this.generation++;
    this.epochVerified = false;
    this.selection = null;
    this.armed = null;
  }

  release(tab: string): void { if ((this.selection ?? this.armed)?.tab === tab) this.dispose(); }
}

export const chatResponsivenessTrace = new ChatResponsivenessTrace();

declare global { interface Window { __chatResponsiveness?: Pick<ChatResponsivenessTrace, "arm" | "select" | "snapshot" | "dispose"> } }

if (DEV_BUILD && typeof window !== "undefined") {
  window.__chatResponsiveness = {
    arm: binding => chatResponsivenessTrace.arm(binding),
    select: selection => chatResponsivenessTrace.select(selection),
    snapshot: () => chatResponsivenessTrace.snapshot(),
    dispose: () => chatResponsivenessTrace.dispose(),
  };
}
