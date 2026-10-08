import { formatAge } from "./timeLabels";

export interface LastActivePeer {
  instance_id: string;
  instance_name: string;
  last_active_ms?: number | null;
}

export function rankPeersByLastActive<T extends LastActivePeer>(peers: T[]): T[] {
  return [...peers].sort((a, b) => {
    const am = a.last_active_ms ?? 0;
    const bm = b.last_active_ms ?? 0;
    if (am !== bm) return bm - am;
    return a.instance_name.localeCompare(b.instance_name);
  });
}

export function formatLastActiveBadge(
  lastActiveMs: number | null | undefined,
  nowMs = Date.now(),
): string | null {
  if (lastActiveMs == null || lastActiveMs <= 0) return null;
  return nowMs - lastActiveMs < 60_000 ? "active now" : `active ${formatAge(lastActiveMs, nowMs)}`;
}
