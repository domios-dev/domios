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
  /** Backend recovery coordinates from the last `snapshot_arrival` (re-attach).
   *  Not a projection stage — kept off `firstStalledEdge`; used to attribute what
   *  a reactivated snapshot recovered to versus the pane's prior committed tail. */
  snapshotTail: string | null;
  snapshotCursor: string | null;
  snapshotOffset: number | null;
}

/** Compact history retained when a pane is forgotten, so a later reactivation can
 *  be causally compared to where the torn-down pane actually left off. This is
 *  archival history, not live pane state: it is never returned by `snapshot`,
 *  never counted by `trackedPaneCount`, and never walked by `firstStalledEdge`. */
export interface PriorActivation {
  committedTail: string | null;
  paintedTail: string | null;
  snapshotTail: string | null;
  snapshotCursor: string | null;
  snapshotOffset: number | null;
}

/** How a reactivated snapshot lines up with the forgotten pane's prior committed
 *  tail. `no_prior`: pane was never torn down. `pending`: torn down, no reactivation
 *  snapshot observed yet. `recovered`: same tail id and content length ≥ prior (clean
 *  cursor/signature-gated reconcile). `regressed`: same id but shorter (the re-attach
 *  came back to earlier content — a desync). `diverged`: a different tail id (needs
 *  the archived cursor/offset to attribute further). */
export type ActivationRecovery = "no_prior" | "pending" | "recovered" | "regressed" | "diverged";

export interface StalledEdge {
  from: ProjectionStage;
  to: ProjectionStage;
  /** Tail signature the upstream stage reached. */
  expectedTail: string | null;
  /** Tail signature the downstream stage has actually caught up to. */
  observedTail: string | null;
}

const DEFAULT_MAX_TRACKED_PANES = 64;

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

interface StageTail {
  stage: ProjectionStage;
  tail: string | null;
}

/** Bounded (LRU) collection of monotonic per-pane projection counters plus the
 *  read seam that identifies the first stalled edge. Counts only ever increase;
 *  tails track the newest message id seen at each stage so a legitimately
 *  collapsed paint (many commits → one visible update) is not mistaken for a
 *  stall — only a tail divergence is. */
export class ChatProjectionMonitor {
  private readonly panes = new Map<string, ProjectionCounters>();
  private readonly priors = new Map<string, PriorActivation>();
  private readonly maxTrackedPanes: number;

  constructor(maxTrackedPanes = DEFAULT_MAX_TRACKED_PANES) {
    this.maxTrackedPanes = Math.max(1, maxTrackedPanes);
  }

  /** A `ChatSessionDiagnosticSink` — pass straight to `useChatSession`. */
  readonly sink: ChatSessionDiagnosticSink = (event) => this.ingest(event);

  ingest(event: ChatSessionDiagnosticEvent): void {
    const counters = this.touch(event.paneId);
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
        counters.snapshotTail = event.tailSignature ?? counters.snapshotTail;
        counters.snapshotCursor = event.cursor ?? counters.snapshotCursor;
        counters.snapshotOffset = event.offset ?? counters.snapshotOffset;
        break;
      default:
        break;
    }
  }

  /** Painted tail from the render layer. `tailSignature` is the last visible
   *  message's signature (`<id>#<contentLength>`; null when the visible
   *  projection is empty). Passive observation only — records what was painted,
   *  never influences what renders. */
  recordPaint(paneId: string, tailSignature: string | null): void {
    const counters = this.touch(paneId);
    counters.painted += 1;
    counters.paintedTail = tailSignature;
  }

  /** Drop a pane's LIVE record on unmount so tracking follows component lifecycle
   *  and torn-down panes do not linger until the LRU cap evicts them. Before
   *  dropping it, archive a compact prior-activation summary (tails + recovery
   *  coordinates) so a later reactivation of the same pane can be causally
   *  compared. The archive is not live state: `snapshot`, `trackedPaneCount`, and
   *  `firstStalledEdge` all ignore it. */
  forget(paneId: string): void {
    const counters = this.panes.get(paneId);
    if (counters) {
      this.priors.delete(paneId);
      this.priors.set(paneId, {
        committedTail: counters.committedTail,
        paintedTail: counters.paintedTail,
        snapshotTail: counters.snapshotTail,
        snapshotCursor: counters.snapshotCursor,
        snapshotOffset: counters.snapshotOffset,
      });
      while (this.priors.size > this.maxTrackedPanes) {
        const oldest = this.priors.keys().next().value;
        if (oldest === undefined) break;
        this.priors.delete(oldest);
      }
    }
    this.panes.delete(paneId);
  }

  snapshot(paneId: string): ProjectionCounters | null {
    const counters = this.panes.get(paneId);
    return counters ? { ...counters } : null;
  }

  /** Archived history from the last time this pane was forgotten (deactivated /
   *  switched to Terminal), or null if it was never torn down. Distinct from
   *  `snapshot`, which only sees the live record. */
  priorActivation(paneId: string): PriorActivation | null {
    const prior = this.priors.get(paneId);
    return prior ? { ...prior } : null;
  }

  /** Attribute how the pane's current (reactivated) snapshot recovered relative to
   *  the tail it had committed before it was forgotten. Compares the live
   *  `snapshot_arrival` signature against the archived prior committed tail so an
   *  away→back / Terminal→Chat reconcile is causally observable. */
  activationRecovery(paneId: string): ActivationRecovery {
    const prior = this.priors.get(paneId);
    if (!prior) return "no_prior";
    const recovered = this.panes.get(paneId)?.snapshotTail ?? null;
    if (recovered == null) return "pending";
    const target = prior.committedTail;
    if (target == null) return "recovered";
    return classifyRecovery(recovered, target);
  }

  /** First stage boundary where the downstream tail lags the upstream tail, in
   *  causal order. Returns null when the whole chain has caught up (or nothing
   *  has arrived yet). The frozen-tail signature surfaces here as a
   *  `committed_state → painted_tail` edge. */
  firstStalledEdge(paneId: string): StalledEdge | null {
    const counters = this.panes.get(paneId);
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

  trackedPaneCount(): number {
    return this.panes.size;
  }

  private touch(paneId: string): ProjectionCounters {
    const existing = this.panes.get(paneId);
    if (existing) {
      // Move to most-recently-used end of the insertion-ordered map.
      this.panes.delete(paneId);
      this.panes.set(paneId, existing);
      return existing;
    }
    const counters = freshCounters();
    this.panes.set(paneId, counters);
    while (this.panes.size > this.maxTrackedPanes) {
      const oldest = this.panes.keys().next().value;
      if (oldest === undefined) break;
      this.panes.delete(oldest);
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

/** Process-wide monitor fed by production panes. Tests construct isolated
 *  `ChatProjectionMonitor` instances instead of sharing this one. */
export const chatProjectionMonitor = new ChatProjectionMonitor();

declare global {
  interface Window {
    __chatProjection?: ChatProjectionMonitor;
  }
}
