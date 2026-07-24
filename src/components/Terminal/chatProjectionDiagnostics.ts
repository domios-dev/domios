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
  arrivalTail: string | null;
  authorityTail: string | null;
  committedTail: string | null;
  paintedTail: string | null;
}

export interface StalledEdge {
  from: ProjectionStage;
  to: ProjectionStage;
  /** Tail the upstream stage reached. */
  expectedTail: string | null;
  /** Tail the downstream stage has actually caught up to. */
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

function tailOf(messageIds: readonly string[], previous: string | null): string | null {
  return messageIds.length > 0 ? messageIds[messageIds.length - 1]! : previous;
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
        counters.arrivalTail = tailOf(event.messageIds, counters.arrivalTail);
        break;
      case "authority_adopt":
        counters.adopted += 1;
        counters.authorityTail = tailOf(event.messageIds, counters.authorityTail);
        break;
      case "authority_drop":
        counters.dropped += 1;
        break;
      case "state_commit":
        counters.committed += 1;
        counters.committedTail = tailOf(event.messageIds, counters.committedTail);
        break;
      default:
        break;
    }
  }

  /** Painted tail from the render layer. `tailId` is the last visible message id
   *  (null when the visible projection is empty). Passive observation only. */
  recordPaint(paneId: string, tailId: string | null): void {
    const counters = this.touch(paneId);
    counters.painted += 1;
    counters.paintedTail = tailId;
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

/** Process-wide monitor fed by production panes. Tests construct isolated
 *  `ChatProjectionMonitor` instances instead of sharing this one. */
export const chatProjectionMonitor = new ChatProjectionMonitor();

declare global {
  interface Window {
    __chatProjection?: ChatProjectionMonitor;
  }
}
