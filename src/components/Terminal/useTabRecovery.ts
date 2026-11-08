import type { ErrorNoticeAction } from "../shared/ErrorNotice";
import type { TabRecovery } from "../../lib/tabFailure";
import { useRestartTab } from "./useRestartTab";
import { useStartNewSession } from "./useStartNewSession";

/** The one mapping from a failure's recovery to the action a surface offers. */
export function useTabRecovery(tabId: string | undefined, recovery: TabRecovery | undefined) {
  const restart = useRestartTab(tabId);
  const fresh = useStartNewSession(tabId ?? "");
  const actions: ErrorNoticeAction[] = recovery === "restart"
    ? [{ label: restart.restarting ? "Restarting…" : "Restart", ariaLabel: "Restart the agent", disabled: restart.restarting, onClick: () => void restart.restart() }]
    : recovery === "start_new_session"
      ? [{ label: fresh.startingNewSession ? "Starting…" : "Start new session", ariaLabel: "Start new session", disabled: fresh.startingNewSession, onClick: () => void fresh.onStartNewSession() }]
      : [];
  return { restart, fresh, actions, error: restart.error ?? fresh.actionError };
}
