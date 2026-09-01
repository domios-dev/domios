// Repository watch lifecycle (codeterm#336 part C; backend half in
// src-tauri/src/repowatch). Invalidation is event- or action-driven, never timed.
import { invoke, listen, type UnlistenFn } from "./bridge";
import { adoptRepoIdentity, invalidatePluginBubblesForRepo } from "./pluginBubbleCache";

export type RepoWatchReason =
  | "not_a_repository"
  | "repository_gone"
  | "watch_refused"
  | "backend_failed"
  | "subscribe_failed"
  | "stream_unavailable"
  | "malformed_response";

export type RepoWatchState =
  | { status: "pending" }
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

type ListenFn = <T>(event: string, handler: (e: { payload: T }) => void) => Promise<UnlistenFn>;

const REASONS: RepoWatchReason[] = [
  "not_a_repository",
  "repository_gone",
  "watch_refused",
  "backend_failed",
  "subscribe_failed",
  "stream_unavailable",
  "malformed_response",
];

function normalizeReason(raw: unknown): RepoWatchReason {
  return REASONS.includes(raw as RepoWatchReason) ? (raw as RepoWatchReason) : "watch_refused";
}

/** Both listeners or neither: a failure on the second must not strand the
 * first, or the process leaks a listener on every retry. */
export async function openRepoWatchStream(
  subscribe: ListenFn,
  handler: (event: RepoWatchEvent) => void,
): Promise<UnlistenFn> {
  const offChanged = await subscribe<{ identity?: string }>("repository-changed", (e) => {
    const identity = e.payload?.identity;
    if (identity) handler({ kind: "changed", identity });
  });
  try {
    const offUnavailable = await subscribe<{ identity?: string; reason?: string }>(
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
  } catch (e) {
    offChanged();
    throw e;
  }
}

const defaultClient: RepoWatchClient = {
  subscribe: (cwd) => invoke<RepoWatchSubscribeResult>("repo_watch_subscribe", { cwd }),
  unsubscribe: (token) => invoke<unknown>("repo_watch_unsubscribe", { token }),
  listen: (handler) => openRepoWatchStream(listen, handler),
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
let streamFailed = false;

function handleEvent(event: RepoWatchEvent): void {
  if (event.kind === "changed") {
    // One call for the identity, not one per lease: N tabs on one repo
    // recompute the shared cache entry exactly once.
    for (const lease of leases) {
      if (lease.identity === event.identity) {
        invalidatePluginBubblesForRepo(event.identity);
        return;
      }
    }
    return;
  }
  for (const lease of leases) {
    if (lease.identity === event.identity && !lease.released) {
      lease.onState({ status: "unavailable", reason: event.reason });
    }
  }
}

function failEveryLease(reason: RepoWatchReason): void {
  for (const lease of [...leases]) {
    if (!lease.released) lease.onState({ status: "unavailable", reason });
  }
}

function ensureStream(): void {
  if (stream) return;
  const attempt = client.listen(handleEvent);
  stream = attempt;
  attempt.then(
    () => {
      if (stream !== attempt) return;
      streamFailed = false;
      for (const lease of leases) {
        if (!lease.released && lease.token && lease.identity) {
          lease.onState({ status: "watching", identity: lease.identity });
        }
      }
    },
    () => {
      // Drop the rejected promise so a later lease opens a fresh stream, and
      // never let a lease claim `watching` while nothing is listening.
      if (stream === attempt) stream = null;
      streamFailed = true;
      failEveryLease("stream_unavailable");
    },
  );
}

function stopStreamIfIdle(): void {
  if (leases.size > 0) return;
  streamFailed = false;
  if (!stream) return;
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
  onState({ status: "pending" });
  ensureStream();

  client.subscribe(cwd).then(
    (result) => {
      const token = typeof result?.token === "string" && result.token ? result.token : null;
      const identity = typeof result?.identity === "string" && result.identity ? result.identity : null;
      if (result?.status !== "watching") {
        leases.delete(lease);
        stopStreamIfIdle();
        if (!lease.released) {
          onState({ status: "unavailable", reason: normalizeReason(result?.reason) });
        }
        return;
      }
      if (!token || !identity) {
        // Without both, no event could ever match this lease. Hand the
        // reference back rather than render a watch that hears nothing.
        leases.delete(lease);
        stopStreamIfIdle();
        if (token) void client.unsubscribe(token).catch(() => {});
        if (!lease.released) {
          onState({ status: "unavailable", reason: "malformed_response" });
        }
        return;
      }
      if (lease.released) {
        void client.unsubscribe(token).catch(() => {});
        return;
      }
      lease.token = token;
      lease.identity = identity;
      adoptRepoIdentity(cwd, identity);
      onState(
        streamFailed
          ? { status: "unavailable", reason: "stream_unavailable" }
          : { status: "watching", identity },
      );
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

export function __repoWatchDebugForTest(): { leases: number; streamOpen: boolean } {
  return { leases: leases.size, streamOpen: stream !== null };
}

export function __resetRepoWatchForTest(): void {
  leases.clear();
  stream = null;
  streamFailed = false;
  client = defaultClient;
}
