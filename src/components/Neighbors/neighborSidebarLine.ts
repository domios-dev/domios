import { friendReceivingLine } from "./neighborFriendReceiving";
import type { ReceiverState } from "./neighborRuntimeClient";
import { conciergeStatusLine, type ConciergeStatus } from "./useConciergeStatus";
import type { TabActivity } from "../../lib/tabActivity";

export function neighborSidebarLine(receiver: ReceiverState | null, localKnown: boolean, lifecycle: ConciergeStatus | null,
  activity: Pick<TabActivity, "kind" | "label"> | null, paused: boolean, paid: boolean): string {
  if (receiver && !receiver.receiving_here) return friendReceivingLine(receiver);
  if (paused) return "disabled";
  if (!receiver && !localKnown) return friendReceivingLine(null);
  if (!paid) return "paused by plan";
  if (!localKnown) return conciergeStatusLine(lifecycle);
  if (lifecycle && !["ready", "not_receiving", "receiving_elsewhere"].includes(lifecycle.state)) return conciergeStatusLine(lifecycle);
  if (activity?.kind === "failed" || activity?.kind === "waiting-on-user") return `needs attention · ${activity.label}`;
  if (activity?.kind === "starting") return "concierge starting";
  return activity?.kind === "working" ? "working" : "idle";
}
