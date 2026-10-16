import type { ChatScrollGesture } from "./chatScrollGesture";

export const TRACE_FRAME_LIMIT = 600;
export const TRACE_EDGE_LIMIT = 400;
export const TRACE_CHUNK_BYTES = 3900;
export const traceEdges = ["touchstart", "touchend", "scroll", "scrollend", "gesture", "write", "absorb", "resize", "clientHeight", "visualViewport", "capabilities"] as const;
export type TraceEdge = typeof traceEdges[number];
export type TraceTuple = number[];
export interface TraceChunk {
  gestureId: string; scopeId: number; index: number; count: number; frames: TraceTuple[]; edges: TraceTuple[];
  droppedFrames: number; droppedEdges: number; droppedBatches: number;
}

export function packGesture(state: ChatScrollGesture): number {
  return Number(state.touching) | Number(state.scrolling) << 1 | Number(state.awaitingMovement) << 2 | Number(state.observedScroll) << 3;
}
export function unpackGesture(bits: number, stationaryFrames: number): ChatScrollGesture {
  return { touching: !!(bits & 1), scrolling: !!(bits & 2), awaitingMovement: !!(bits & 4), observedScroll: !!(bits & 8), stationaryFrames };
}
export function traceNumber(value: number): number {
  return Number.isFinite(value) ? Math.round(Math.max(-1e12, Math.min(1e12, value)) * 100) / 100 : 0;
}

export class ChatGestureTrace {
  readonly frames: TraceTuple[] = [];
  readonly edges: TraceTuple[] = [];
  droppedFrames = 0;
  droppedEdges = 0;
  frame = 0;
  private tail = 0;
  private totalFrames = 0;
  readonly gestureId: string;
  readonly scopeId: number;
  constructor(gestureId: string, scopeId = 0) {
    this.gestureId = /^[a-zA-Z0-9_.-]{1,64}$/.test(gestureId) ? gestureId : "unknown";
    this.scopeId = traceNumber(scopeId);
  }

  sample(values: readonly number[], owned: boolean): boolean {
    this.frame = this.totalFrames++;
    if (this.frames.length === TRACE_FRAME_LIMIT) { this.frames.shift(); this.droppedFrames++; }
    this.frames.push([this.frame, ...values.slice(0, 16).map(traceNumber)]);
    this.tail = owned ? 0 : this.tail + 1;
    return this.tail >= 60;
  }
  edge(edge: TraceEdge, values: readonly number[] = []): void {
    if (this.edges.length === TRACE_EDGE_LIMIT) { this.edges.shift(); this.droppedEdges++; }
    this.edges.push([this.frame, traceEdges.indexOf(edge), ...values.slice(0, 34).map(traceNumber)]);
  }
  chunks(droppedBatches = 0): TraceChunk[] {
    const chunks: TraceChunk[] = [];
    const empty = (): TraceChunk => ({ gestureId: this.gestureId, scopeId: this.scopeId, index: 0, count: 0, frames: [], edges: [],
      droppedFrames: this.droppedFrames, droppedEdges: this.droppedEdges, droppedBatches });
    let chunk = empty();
    const bytes = (value: TraceChunk) => new TextEncoder().encode(JSON.stringify({ scope: "trace", edge: "trace", at: 0, state: value })).length;
    let overflowFrames = 0;
    let overflowEdges = 0;
    for (const kind of ["frames", "edges"] as const) for (const tuple of this[kind]) {
      chunk[kind].push(tuple);
      if (bytes(chunk) > TRACE_CHUNK_BYTES - 32) {
        chunk[kind].pop();
        if (chunks.length === 31) {
          if (kind === "frames") overflowFrames++; else overflowEdges++;
          continue;
        }
        chunks.push(chunk); chunk = empty(); chunk[kind].push(tuple);
      }
    }
    chunks.push(chunk);
    return chunks.map((value, index) => ({ ...value, index, count: chunks.length, droppedFrames: value.droppedFrames + overflowFrames, droppedEdges: value.droppedEdges + overflowEdges }));
  }
}
