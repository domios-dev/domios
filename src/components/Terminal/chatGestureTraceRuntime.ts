import { DEV_BUILD } from "../../lib/devBuild";
import { diagnosticBuildSha } from "../../lib/frontendDiagnosticStamp";
import { buildHash } from "../../lib/buildStamp";
import { baseDaemonTransport } from "../../lib/daemonBootstrap";
import { ChatGestureTrace, continueChatGestureTrace, packGesture, TRACE_EDGE_LIMIT, TRACE_FRAME_LIMIT, type TraceEdge } from "./chatGestureTrace";
import { isChatUserScrolling, type ChatScrollGesture } from "./chatScrollGesture";

const probes = new Map<string, () => number[]>();
const traces = new Map<string, ChatGestureTrace>();
const rollover = new Map<string, () => void>();
let sequence = 0;
let droppedBatches = 0;

export function traceIdentity(id: string | undefined): number {
  let hash = 2166136261;
  for (const char of (id ?? "").slice(0, 128)) hash = Math.imul(hash ^ char.charCodeAt(0), 16777619);
  return hash >>> 0;
}
export function registerChatTraceProbe(scope: string, probe: () => number[]): () => void {
  if (!DEV_BUILD) return () => {};
  probes.set(scope, probe);
  return () => { if (probes.get(scope) === probe) probes.delete(scope); };
}
export function chatTraceActive(scope: string): boolean {
  return DEV_BUILD && traces.has(scope);
}
export function chatTraceEdge(scope: string, edge: TraceEdge, values: readonly number[] = []): void {
  if (!DEV_BUILD) return;
  if ((traces.get(scope)?.edges.length ?? 0) >= TRACE_EDGE_LIMIT) rollover.get(scope)?.();
  traces.get(scope)?.edge(edge, values);
}
export function collectChatTraceDiagnostic(scope: string, edge: string, state: object): boolean {
  if (!DEV_BUILD || !traces.has(scope)) return false;
  const value = state as Record<string, unknown>;
  if (edge === "write") chatTraceEdge(scope, "write", [Number(value.before), Number(value.after), Number(value.owned)]);
  if (edge === "jump_ignored_owned") chatTraceEdge(scope, "jump_ignored_owned");
  return true;
}

export function startChatGestureTrace(scope: string, el: HTMLElement, gesture: () => ChatScrollGesture): () => void {
  if (!DEV_BUILD) return () => {};
  const gestureId = `gesture-${++sequence}`;
  let segment = 0;
  let trace = new ChatGestureTrace(gestureId, traceIdentity(scope));
  traces.set(scope, trace);
  trace.edge("touchstart");
  trace.edge("capabilities", [Number("onscrollend" in el), Number(typeof CSS !== "undefined" && CSS.supports("overflow-anchor", "auto"))]);
  let frame: number | undefined;
  let height = el.clientHeight;
  let finished = false;
  const flush = () => {
    if (finished) return;
    finished = true;
    if (frame !== undefined) cancelAnimationFrame(frame);
    window.visualViewport?.removeEventListener("resize", viewportResize);
    el.removeEventListener("scroll", scroll);
    if (traces.get(scope) === trace) traces.delete(scope);
    rollover.delete(scope);
    emit();
  };
  const emit = () => {
    const chunks = trace.chunks(droppedBatches);
    droppedBatches = 0;
    for (const state of chunks) {
      const body = { stage: "chat-scroll", buildSha: diagnosticBuildSha(buildHash()), native: "__TAURI_INTERNALS__" in window, injected: false, mounted: true,
        chat: { scope: "trace", edge: "trace", at: 0, state } };
      void Promise.resolve().then(() => baseDaemonTransport().localJson("POST", "/api/frontend/diagnostic", body, { signal: AbortSignal.timeout(5000) }))
        .catch(() => { droppedBatches = Math.min(0xffffffff, droppedBatches + 1); });
    }
  };
  const rotate = () => {
    emit();
    trace = continueChatGestureTrace(trace, `${gestureId}.${++segment}`);
    traces.set(scope, trace);
  };
  rollover.set(scope, rotate);
  const scroll = () => chatTraceEdge(scope, "scroll", [el.scrollTop, Number(isChatUserScrolling(gesture()))]);
  const viewportResize = () => chatTraceEdge(scope, "visualViewport", [window.visualViewport?.width ?? 0, window.visualViewport?.height ?? 0]);
  const sample = () => {
    if (trace.frames.length >= TRACE_FRAME_LIMIT) rotate();
    const state = gesture();
    if (height !== el.clientHeight) { chatTraceEdge(scope, "clientHeight", [height, el.clientHeight]); height = el.clientHeight; }
    const done = trace.sample([el.scrollTop, el.scrollHeight, el.clientHeight, packGesture(state), state.stationaryFrames,
      ...(probes.get(scope)?.() ?? [0, 0, 0, 0, 0, 0])], isChatUserScrolling(state));
    if (done) flush();
    else frame = requestAnimationFrame(sample);
  };
  window.visualViewport?.addEventListener("resize", viewportResize);
  el.addEventListener("scroll", scroll, { passive: true });
  frame = requestAnimationFrame(sample);
  return flush;
}
