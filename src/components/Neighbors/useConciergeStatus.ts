import { useMemo } from "react";
import type { NeighborRuntimeClient } from "./neighborRuntimeClient";
import { useNeighborStatus } from "./neighborStatusStore";

export interface ConciergeStatus {
  gateway_tab: string | null;
  concierge_missing: boolean;
  /** Null until an inbox count has been observed; never a stand-in zero. */
  pending_count: number | null;
  /** The daemon stops counting at its cap; the real number may be higher. */
  pending_count_capped?: boolean;
  receiving_here: boolean;
  receiver_machine_label: string | null;
  state: "ready" | "concierge_starting" | "concierge_missing" | "receiving_elsewhere" | "not_receiving";
}

const STATES: readonly ConciergeStatus["state"][] = ["ready", "concierge_starting", "concierge_missing", "receiving_elsewhere", "not_receiving"];
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;

export function parseConciergeStatus(value: unknown): ConciergeStatus | null {
  if (!record(value) || typeof value.concierge_missing !== "boolean" || typeof value.receiving_here !== "boolean"
    || !STATES.includes(value.state as ConciergeStatus["state"])
    || (value.gateway_tab !== null && typeof value.gateway_tab !== "string")
    || (value.receiver_machine_label !== null && typeof value.receiver_machine_label !== "string")
    || (value.pending_count !== null && (typeof value.pending_count !== "number" || !Number.isSafeInteger(value.pending_count) || value.pending_count < 0))) return null;
  return {
    gateway_tab: value.gateway_tab, concierge_missing: value.concierge_missing, pending_count: value.pending_count as number | null,
    receiving_here: value.receiving_here, receiver_machine_label: value.receiver_machine_label, state: value.state as ConciergeStatus["state"],
    ...(value.pending_count_capped === true ? { pending_count_capped: true } : {}),
  };
}

/** Each friendship's concierge on this machine, from the runtime status and its per-friendship events. */
export function useConciergeStatus(client: NeighborRuntimeClient, active: boolean): Record<string, ConciergeStatus> {
  const status = useNeighborStatus(client, active);
  return useMemo(() => {
    const parsed: Record<string, ConciergeStatus> = {};
    for (const [friendship, value] of Object.entries(status?.concierges ?? {})) {
      const entry = parseConciergeStatus(value);
      if (entry) parsed[friendship] = entry;
    }
    return parsed;
  }, [status]);
}

// The daemon stops counting the waiting inbox at this many.
const PENDING_CAP = 100;

/** "No concierge on this machine", with the waiting count only once it has been observed. */
export function missingConciergeLine(status: ConciergeStatus): string {
  if (status.pending_count === null || status.pending_count === 0) return "No concierge on this machine";
  const count = status.pending_count_capped || status.pending_count >= PENDING_CAP ? `${status.pending_count}+` : String(status.pending_count);
  return `No concierge on this machine — ${count} ${status.pending_count === 1 ? "message" : "messages"} waiting`;
}
