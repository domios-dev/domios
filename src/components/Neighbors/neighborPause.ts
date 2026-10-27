import { useEffect, useSyncExternalStore } from "react";
import type { NeighborConciergeClient } from "./neighborConciergeClient";

// Every surface reads one record, so pausing in the friend window dims the sidebar row and the node at once.
const changed = new Map<string, boolean>();
const listeners = new Set<() => void>();
const latest = new Map<string, number>();
let version = 0;

function publish(friendship: string, paused: boolean) {
  changed.set(friendship, paused);
  version++;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

/** Whether you paused a friend: the list's value until a pause event or your own change says otherwise. */
export function usePausedFriends(client: NeighborConciergeClient): (friendship: string, listed: boolean | undefined) => boolean {
  useSyncExternalStore(subscribe, () => version);
  useEffect(() => {
    let stop: (() => void) | undefined;
    let live = true;
    void client.onPausedChanged(publish).then(unlisten => { if (live) stop = unlisten; else unlisten(); });
    return () => { live = false; stop?.(); };
  }, [client]);
  return (friendship, listed) => changed.get(friendship) ?? listed ?? false;
}

/** Pauses or resumes at once; only the newest change's answer lands, and a failure restores the state before it. */
export function setFriendPaused(client: NeighborConciergeClient, friendship: string, paused: boolean, current: boolean,
  onError: (message: string) => void) {
  const change = (latest.get(friendship) ?? 0) + 1;
  latest.set(friendship, change);
  publish(friendship, paused);
  void client.setPaused(friendship, paused).then(saved => { if (latest.get(friendship) === change) publish(friendship, saved); }, (failure: unknown) => {
    if (latest.get(friendship) !== change) return;
    publish(friendship, current);
    onError(failure instanceof Error ? failure.message : paused ? "The friend could not be paused." : "The friend could not be resumed.");
  });
}

/** Pausing is one way: it stops their messages to you and leaves yours to them. */
export const pausedLine = (name: string) => `You paused ${name}. They can't write to you until you resume; you can still write to them.`;
export const theyPausedLine = (name: string) => `${name} paused messages from you.`;

/** Forget local pause changes; they belong to the account they were made in. */
export function resetPausedFriends() {
  changed.clear();
  latest.clear();
  version++;
  for (const listener of listeners) listener();
}
