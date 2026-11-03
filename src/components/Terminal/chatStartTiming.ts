import { DEV_BUILD } from "../../lib/devBuild";
import { recordClientHealth } from "../../lib/clientJournal";

type Timing = { id: string; tabId: string; adopted: boolean; received: boolean; started: number; responseAt?: number; adoptedAt?: number; cancelPaint?: () => void };
const pending = new Map<string, Timing>();
let sequence = 0;

export function beginChatStartTiming(tabId: string): Timing | undefined {
  pending.get(tabId)?.cancelPaint?.();
  const timing: Timing = { id: `${tabId}:${++sequence}`, tabId, adopted: false, received: false, started: performance.now() };
  pending.set(tabId, timing);
  if (DEV_BUILD) performance.mark(`chat_start:${timing.id}:request`);
  return timing;
}

export function chatStartTimingEdge(timing: Timing | undefined, edge: "response" | "adopted", response?: unknown): void {
  if (!timing) return;
  if (edge === "response") timing.responseAt = performance.now();
  else timing.adoptedAt = performance.now();
  const elapsedMs = Math.round(performance.now() - timing.started);
  if (edge === "response" && elapsedMs >= 2_000) {
    recordClientHealth({ event: "chat-connect-slow", tabId: timing.tabId, elapsedMs, responseMs: elapsedMs });
  }
  if (DEV_BUILD) {
    performance.mark(`chat_start:${timing.id}:${edge}`);
    const previous = edge === "response" ? "request" : "response";
    performance.measure(`chat_start:${edge}`, {
      start: `chat_start:${timing.id}:${previous}`,
      end: `chat_start:${timing.id}:${edge}`,
      detail: { tabId: timing.tabId, requestId: timing.id,
        ...(edge === "response" ? { responseBytes: new TextEncoder().encode(JSON.stringify(response)).byteLength } : {}) },
    });
  }
  if (edge === "adopted") timing.adopted = true;
  else timing.received = true;
}

export function adoptChatStartTiming(tabId: string): void {
  const timing = pending.get(tabId);
  if (timing?.received && !timing.adopted) chatStartTimingEdge(timing, "adopted");
}

export function paintChatStartTiming(tabId: string): void {
  if (typeof requestAnimationFrame !== "function") return;
  const timing = pending.get(tabId);
  if (!timing?.adopted || timing.cancelPaint) return;
  let second = 0;
  const first = requestAnimationFrame(() => {
    second = requestAnimationFrame(() => {
      if (pending.get(tabId) !== timing) return;
      const elapsedMs = Math.round(performance.now() - timing.started);
      if (elapsedMs >= 2_000) recordClientHealth({ event: "chat-connect-slow", tabId, elapsedMs,
        responseMs: Math.round((timing.responseAt ?? timing.started) - timing.started),
        adoptionMs: Math.round((timing.adoptedAt ?? timing.responseAt ?? timing.started) - (timing.responseAt ?? timing.started)),
        paintMs: Math.round(performance.now() - (timing.adoptedAt ?? timing.started)) });
      if (DEV_BUILD) {
        performance.mark(`chat_start:${timing.id}:painted`);
        for (const [name, start] of [["painted", "adopted"], ["total", "request"]]) {
          performance.measure(`chat_start:${name}`, {
            start: `chat_start:${timing.id}:${start}`, end: `chat_start:${timing.id}:painted`,
            detail: { tabId, requestId: timing.id },
          });
        }
        for (const edge of ["request", "response", "adopted", "painted"]) performance.clearMarks(`chat_start:${timing.id}:${edge}`);
      }
      pending.delete(tabId);
    });
  });
  timing.cancelPaint = () => {
    cancelAnimationFrame(first);
    cancelAnimationFrame(second);
    timing.cancelPaint = undefined;
  };
}

export function cancelChatStartPaintTiming(tabId: string): void {
  pending.get(tabId)?.cancelPaint?.();
  pending.delete(tabId);
}
