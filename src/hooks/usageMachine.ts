import { createContext, useContext } from "react";
import { listen } from "../lib/bridge";
import { peerEventRail } from "../lib/peerEventRail";
import type { UsageSnapshotEvent } from "../types/usage";

export const UsageMachineContext = createContext<string | null | undefined>(undefined);
export const useUsageMachine = () => useContext(UsageMachineContext);

export function scopedUsageArgs(peerId: string | null | undefined) {
  return peerId === undefined ? {} : { peerId };
}

export async function subscribeUsageUpdates(
  peerId: string | null | undefined,
  provider: string,
  accountId: string | null,
  read: (event?: UsageSnapshotEvent) => void,
  invalidate: () => void,
): Promise<() => void> {
  if (!peerId) {
    return listen<UsageSnapshotEvent>("usage-snapshot-updated", ({ payload }) => {
      if (payload.provider === provider && (payload.account_id ?? null) === accountId) read(payload);
    });
  }
  const onFocus = () => read();
  const stop = peerEventRail.observe(peerId, event => {
    if (event.type === "reconcile") read();
    if (event.type === "invalidate") invalidate();
  });
  window.addEventListener("focus", onFocus);
  return () => {
    window.removeEventListener("focus", onFocus);
    stop();
  };
}
