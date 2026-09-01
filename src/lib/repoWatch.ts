// Repository watch lifecycle (codeterm#336 part C, backend half in
// src-tauri/src/repowatch). A visible bubble leases a watch; the daemon
// refcounts one watcher per repository identity and pushes precise change
// events. Nothing here is timed: invalidation happens on an event or an
// explicit Git action, never on elapsed time.
import { invoke, listen, type UnlistenFn } from "./bridge";
import { invalidatePluginBubblesForRepo } from "./pluginBubbleCache";

export type RepoWatchReason =
  | "not_a_repository"
  | "repository_gone"
  | "watch_refused"
  | "subscribe_failed";

export type RepoWatchState =
  | { status: "watching"; identity: string }
  | { status: "unavailable"; reason: RepoWatchReason };

export type RepoWatchEvent =
  | { kind: "changed"; identity: string }
  | { kind: "unavailable"; identity: string; reason: RepoWatchReason };

export interface RepoWatchSubscribeResult {
  status?: string;
  token?: string;
  identity?: string;
  reason?: string;
}

export interface RepoWatchClient {
  subscribe: (cwd: string) => Promise<RepoWatchSubscribeResult>;
  unsubscribe: (token: string) => Promise<unknown>;
  listen: (handler: (event: RepoWatchEvent) => void) => Promise<UnlistenFn>;
}

export interface RepoWatchLease {
  release: () => void;
}

const REASONS: RepoWatchReason[] = [
  "not_a_repository",
  "repository_gone",
  "watch_refused",
  "subscribe_failed",
];

function normalizeReason(raw: unknown): RepoWatchReason {
  return REASONS.includes(raw as RepoWatchReason) ? (raw as RepoWatchReason) : "watch_refused";
}

const defaultClient: RepoWatchClient = {
  subscribe: (cwd) => invoke<RepoWatchSubscribeResult>("repo_watch_subscribe", { cwd }),
  unsubscribe: (token) => invoke<unknown>("repo_watch_unsubscribe", { token }),
  listen: async (handler) => {
    const offChanged = await listen<{ identity?: string }>("repository-changed", (e) => {
      const identity = e.payload?.identity;
      if (identity) handler({ kind: "changed", identity });
    });
    const offUnavailable = await listen<{ identity?: string; reason?: string }>(
      "repository-unavailable",
      (e) => {
        const identity = e.payload?.identity;
        if (identity) {
          handler({ kind: "unavailable", identity, reason: normalizeReason(e.payload?.reason) });
        }
      },
    );
    return () => {
      offChanged();
      offUnavailable();
    };
  },
};

let client: RepoWatchClient = defaultClient;

interface Lease {
  cwd: string;
  identity: string | null;
  token: string | null;
  released: boolean;
  onState: (state: RepoWatchState) => void;
}

const leases = new Set<Lease>();
let stream: Promise<UnlistenFn> | null = null;

function handleEvent(event: RepoWatchEvent): void {
  if (event.kind === "changed") {
    // One invalidation per distinct repo path: ten tabs on one identity
    // recompute the shared cache entry exactly once.
    const roots = new Set<string>();
    for (const lease of leases) if (lease.identity === event.identity) roots.add(lease.cwd);
    for (const cwd of roots) invalidatePluginBubblesForRepo(cwd);
    return;
  }
  for (const lease of leases) {
    if (lease.identity === event.identity && !lease.released) {
      lease.onState({ status: "unavailable", reason: event.reason });
    }
  }
}

function ensureStream(): void {
  if (stream) return;
  stream = client.listen(handleEvent);
  stream.catch(() => {});
}

function stopStreamIfIdle(): void {
  if (leases.size > 0 || !stream) return;
  const pending = stream;
  stream = null;
  void pending.then((off) => off()).catch(() => {});
}

/** Lease a watch for `cwd`. Exactly one release per lease; releasing during an
 * in-flight subscribe still hands the reference back. */
export function watchRepository(
  cwd: string,
  onState: (state: RepoWatchState) => void,
): RepoWatchLease {
  const lease: Lease = { cwd, identity: null, token: null, released: false, onState };
  leases.add(lease);
  ensureStream();

  client.subscribe(cwd).then(
    (result) => {
      if (result?.status !== "watching" || !result.token) {
        leases.delete(lease);
        stopStreamIfIdle();
        if (!lease.released) {
          onState({ status: "unavailable", reason: normalizeReason(result?.reason) });
        }
        return;
      }
      if (lease.released) {
        void client.unsubscribe(result.token).catch(() => {});
        return;
      }
      lease.token = result.token;
      lease.identity = result.identity ?? null;
      onState({ status: "watching", identity: lease.identity ?? cwd });
    },
    () => {
      leases.delete(lease);
      stopStreamIfIdle();
      if (!lease.released) onState({ status: "unavailable", reason: "subscribe_failed" });
    },
  );

  return {
    release: () => {
      if (lease.released) return;
      lease.released = true;
      leases.delete(lease);
      stopStreamIfIdle();
      const token = lease.token;
      lease.token = null;
      if (token) void client.unsubscribe(token).catch(() => {});
    },
  };
}

export function __setRepoWatchClientForTest(next: RepoWatchClient | null): void {
  client = next ?? defaultClient;
}

export function __resetRepoWatchForTest(): void {
  leases.clear();
  stream = null;
  client = defaultClient;
}
