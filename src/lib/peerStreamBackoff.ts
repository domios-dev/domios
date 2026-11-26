export const PEER_STREAM_BACKOFF_FIRST_MS = 1_000;
export const PEER_STREAM_BACKOFF_CAP_MS = 30_000;
export const PEER_STREAM_REFRESH_COOLDOWN_MS = 10_000;

export interface PeerStreamBackoffOptions {
  now?: () => number;
  random?: () => number;
  firstMs?: number;
  capMs?: number;
  refreshCooldownMs?: number;
}

export interface PeerStreamBackoffDecision {
  delayMs: number;
  nextRetryAt: number;
  attempts: number;
  shouldRefresh: boolean;
}

export interface PeerStreamBackoff {
  recordRefusal(): PeerStreamBackoffDecision;
  remainingMs(): number;
  isBlocked(): boolean;
  attempts(): number;
  reset(): void;
}

export function computePeerStreamBackoffMs(
  attempts: number,
  random: number,
  firstMs = PEER_STREAM_BACKOFF_FIRST_MS,
  capMs = PEER_STREAM_BACKOFF_CAP_MS,
): number {
  const step = Math.max(1, attempts);
  const ceiling = Math.min(capMs, firstMs * 2 ** Math.min(step - 1, 16));
  const unit = Number.isFinite(random) ? Math.min(1, Math.max(0, random)) : 0.5;
  return Math.max(1, Math.round(ceiling * (0.5 + 0.5 * unit)));
}

export function createPeerStreamBackoff(options: PeerStreamBackoffOptions = {}): PeerStreamBackoff {
  const now = options.now ?? (() => Date.now());
  const random = options.random ?? (() => Math.random());
  const firstMs = Math.max(1, options.firstMs ?? PEER_STREAM_BACKOFF_FIRST_MS);
  const capMs = Math.max(firstMs, options.capMs ?? PEER_STREAM_BACKOFF_CAP_MS);
  const refreshCooldownMs = Math.max(1, options.refreshCooldownMs ?? PEER_STREAM_REFRESH_COOLDOWN_MS);

  let attemptCount = 0;
  let nextRetryAt = 0;
  let lastRefreshAt: number | null = null;

  return {
    recordRefusal() {
      const at = now();
      attemptCount += 1;
      const delayMs = computePeerStreamBackoffMs(attemptCount, random(), firstMs, capMs);
      nextRetryAt = at + delayMs;
      const shouldRefresh = lastRefreshAt === null || at - lastRefreshAt >= refreshCooldownMs;
      if (shouldRefresh) lastRefreshAt = at;
      return { delayMs, nextRetryAt, attempts: attemptCount, shouldRefresh };
    },
    remainingMs() {
      return Math.max(0, nextRetryAt - now());
    },
    isBlocked() {
      return nextRetryAt > now();
    },
    attempts: () => attemptCount,
    reset() {
      attemptCount = 0;
      nextRetryAt = 0;
      lastRefreshAt = null;
    },
  };
}
