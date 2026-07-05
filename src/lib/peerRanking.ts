export interface LastActivePeer {
  instance_id: string;
  instance_name: string;
  last_active_ms?: number | null;
}

const RECENCY_MS = 30 * 60 * 1000;
const STALE_MS = 60 * 60 * 1000;
const LOCAL_MARGIN_MS = 60 * 1000;

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
  const ageMs = nowMs - lastActiveMs;
  if (ageMs < 60_000) return "active now";
  const mins = Math.floor(ageMs / 60_000);
  if (mins < 60) return `active ${mins}m ago`;
  const hours = Math.floor(mins / 60);
  return `active ${hours}h ago`;
}

export function pickResumePeer<T extends LastActivePeer>(
  peers: T[],
  localLastActiveMs: number | null | undefined,
  localDev: boolean,
  opts: {
    hasRelayTarget: boolean;
    dismissedThisSession: boolean;
    nowMs?: number;
  },
): T | null {
  if (opts.hasRelayTarget || opts.dismissedThisSession || localDev) return null;
  if (peers.length === 0) return null;
  const nowMs = opts.nowMs ?? Date.now();
  const ranked = rankPeersByLastActive(peers.filter((p) => p.last_active_ms != null));
  const best = ranked[0];
  if (!best?.last_active_ms) return null;
  const ageMs = nowMs - best.last_active_ms;
  if (ageMs > STALE_MS || ageMs > RECENCY_MS) return null;
  const local = localLastActiveMs ?? 0;
  if (best.last_active_ms <= local + LOCAL_MARGIN_MS) return null;
  return best;
}
