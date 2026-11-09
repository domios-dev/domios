import { useEffect } from "react";
import { useRestartTab } from "./useRestartTab";

/** Restarts the tab's agent on its own session, so a failed runtime recovers from the status row. */
export function TabRecoveryAction({ tabId, onError }: { tabId: string; onError: (message: string) => void }) {
  const { restart, restarting, error } = useRestartTab(tabId);
  useEffect(() => { if (error) onError(error); }, [error, onError]);
  return <button type="button" data-testid="agent-restart" disabled={restarting} onClick={() => void restart()}
    className="shrink-0 rounded-md px-1.5 py-0.5 text-xs font-medium disabled:opacity-50"
    aria-label="Restart the agent" title="Restart the agent">
    {restarting ? "Restarting…" : "Restart"}
  </button>;
}
