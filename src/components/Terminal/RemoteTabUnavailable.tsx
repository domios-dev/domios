import type { Tab } from "../../lib/types";

export function RemoteTabUnavailable({ tab }: { tab: Tab }) {
  const state = tab.remote_availability === "reconnecting" ? "Reconnecting" : "Unavailable";
  return (
    <div role="status" aria-live="polite" data-testid="remote-tab-unavailable" className="flex h-full items-center justify-center text-sm text-[var(--ct-muted-fg)]">
      {state} · @{tab.remote_peer_name ?? tab.remote_peer_id}
    </div>
  );
}
