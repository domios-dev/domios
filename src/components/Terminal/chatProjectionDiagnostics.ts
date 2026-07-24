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
}

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
  };
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

  /** Drop a pane's record on unmount so tracking follows component lifecycle and
   *  torn-down panes do not linger until the LRU cap evicts them. */
  forget(paneId: string): void {
    this.panes.delete(paneId);
  }

  snapshot(paneId: string): ProjectionCounters | null {
    const counters = this.panes.get(paneId);
    return counters ? { ...counters } : null;
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
