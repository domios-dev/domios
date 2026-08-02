import type { Tab } from "../../lib/types";

export function tabContentSurface(tab: Tab): "ordinary" | "remote-unavailable" {
  return tab.remote_peer_id && tab.remote_availability !== "available"
    ? "remote-unavailable"
    : "ordinary";
}
