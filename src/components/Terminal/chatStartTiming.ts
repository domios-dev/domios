import { DEV_BUILD } from "../../lib/devBuild";

type Timing = { id: string; tabId: string; adopted: boolean; received: boolean; cancelPaint?: () => void };
const pending = new Map<string, Timing>();
let sequence = 0;

export function beginChatStartTiming(tabId: string): Timing | undefined {
  if (!DEV_BUILD) return;
  pending.get(tabId)?.cancelPaint?.();
  const timing: Timing = { id: `${tabId}:${++sequence}`, tabId, adopted: false, received: false };
  pending.set(tabId, timing);
  performance.mark(`chat_start:${timing.id}:request`);
  return timing;
}

export function chatStartTimingEdge(timing: Timing | undefined, edge: "response" | "adopted", response?: unknown): void {
  if (!DEV_BUILD || !timing) return;
  performance.mark(`chat_start:${timing.id}:${edge}`);
  const previous = edge === "response" ? "request" : "response";
  performance.measure(`chat_start:${edge}`, {
    start: `chat_start:${timing.id}:${previous}`,
    end: `chat_start:${timing.id}:${edge}`,
    detail: { tabId: timing.tabId, requestId: timing.id,
      ...(edge === "response" ? { responseBytes: new TextEncoder().encode(JSON.stringify(response)).byteLength } : {}) },
  });
  if (edge === "adopted") timing.adopted = true;
  else timing.received = true;
}

export function adoptChatStartTiming(tabId: string): void {
  if (!DEV_BUILD) return;
  const timing = pending.get(tabId);
  if (timing?.received && !timing.adopted) chatStartTimingEdge(timing, "adopted");
}

export function paintChatStartTiming(tabId: string): void {
  if (!DEV_BUILD) return;
  const timing = pending.get(tabId);
  if (!timing?.adopted || timing.cancelPaint) return;
  let second = 0;
  const first = requestAnimationFrame(() => {
    second = requestAnimationFrame(() => {
      if (pending.get(tabId) !== timing) return;
      performance.mark(`chat_start:${timing.id}:painted`);
      for (const [name, start] of [["painted", "adopted"], ["total", "request"]]) {
        performance.measure(`chat_start:${name}`, {
          start: `chat_start:${timing.id}:${start}`, end: `chat_start:${timing.id}:painted`,
          detail: { tabId, requestId: timing.id },
        });
      }
      for (const edge of ["request", "response", "adopted", "painted"]) performance.clearMarks(`chat_start:${timing.id}:${edge}`);
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
  if (DEV_BUILD) pending.get(tabId)?.cancelPaint?.();
}
