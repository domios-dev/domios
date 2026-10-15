import { useCallback, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { CronJob, MultiplexerState, Notification } from "../../lib/types";
import type { TeamCloseInventory } from "../../lib/teamActions";
import { readWorkspaceBlueprints } from "../Automation/blueprintOwner";
import { startBackgroundClose, teamCloseOutcome, teamCloseSurvivors } from "./teamCloseOutcome";

type Notice = Omit<Notification, "id" | "timestamp" | "read">;
export function useTeamClose(close: ((teamId: string) => Promise<TeamCloseInventory>) | undefined, notify: (notice: Notice) => void) {
  const [closingTeamIds, setClosingTeamIds] = useState<ReadonlySet<string>>(new Set());
  const run = useCallback(async function run(teamId: string, workspaceId: string, dismiss: () => void) {
    if (!close) return;
    setClosingTeamIds(ids => new Set([...ids, teamId]));
    let inventory: TeamCloseInventory = { removed: [], kept: [], orphan_branches: [] };
    let survivors: string[] = [];
    let verificationUnavailable = false;
    let closeFailure: unknown;
    try {
      inventory = await startBackgroundClose(dismiss, () => close(teamId));
    } catch (error) {
      closeFailure = error;
      const [stateRead, cronRead, blueprintRead] = await Promise.allSettled([
        invoke<MultiplexerState>("mux_get_state"),
        invoke<CronJob[]>("cron_list", {}),
        readWorkspaceBlueprints(workspaceId),
      ]);
      const state = stateRead.status === "fulfilled" ? stateRead.value : null;
      const crons = cronRead.status === "fulfilled" ? cronRead.value : [];
      const blueprints = blueprintRead.status === "fulfilled" ? blueprintRead.value : [];
      survivors = teamCloseSurvivors(teamId, Object.values(state?.tabs ?? {}), crons, blueprints);
      verificationUnavailable = [stateRead, cronRead, blueprintRead].some(read => read.status === "rejected");
    } finally {
      setClosingTeamIds(ids => new Set([...ids].filter(id => id !== teamId)));
    }
    const outcome = teamCloseOutcome(inventory, survivors);
    if (outcome.kind === "closed" && !verificationUnavailable) return;
    notify({
      tab_id: null, workspace_id: workspaceId,
      title: outcome.kind === "partial" ? "Still open" : verificationUnavailable ? "Close status unavailable" : "Lanes preserved",
      body: [verificationUnavailable && outcome.kind !== "partial" ? "The close could not be verified. Retry to finish closing." : (outcome.kind === "partial" ? outcome.survivors : outcome.preserved).join("\n"), closeFailure instanceof Error ? closeFailure.message : closeFailure == null ? "" : String(closeFailure)].filter(Boolean).join("\n"),
      severity: outcome.kind === "partial" || verificationUnavailable ? "Error" : "Info",
      level: outcome.kind === "partial" || verificationUnavailable ? "failure" : "verbose",
      sticky: outcome.kind === "partial",
      ...(outcome.kind === "partial" || verificationUnavailable ? { local_actions: [{ label: "Retry", run: () => run(teamId, workspaceId, () => {}) }] } : {}),
    });
  }, [close, notify]);
  return { closingTeamIds, closeTeam: run };
}
