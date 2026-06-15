import type { NormalizedChatMessage } from "./types";

function timestampMs(ts: string | undefined): number | null {
  if (!ts) return null;
  const numeric = Number(ts);
  if (Number.isFinite(numeric)) return numeric;
  const parsed = Date.parse(ts);
  return Number.isNaN(parsed) ? null : parsed;
}

export function mergeTimelineMessages<T extends NormalizedChatMessage>(
  confirmed: T[],
  pending: T[],
): T[] {
  if (pending.length === 0) return confirmed;
  return [
    ...confirmed.map((message, index) => ({ message, index })),
    ...pending.map((message, index) => ({ message, index: confirmed.length + index })),
  ]
    .sort((a, b) => {
      const at = timestampMs(a.message.timestamp);
      const bt = timestampMs(b.message.timestamp);
      if (at != null && bt != null && at !== bt) return at - bt;
      return a.index - b.index;
    })
    .map((entry) => entry.message);
}
