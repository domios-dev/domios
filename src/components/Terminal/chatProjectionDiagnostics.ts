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
