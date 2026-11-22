import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { CronJob, MultiplexerState } from "../../lib/types";
import type { TeamCloseInventory } from "../../lib/teamActions";
import { readWorkspaceBlueprints } from "../Automation/blueprintOwner";
import { startBackgroundClose, teamCloseOutcome, teamCloseSurvivors } from "./teamCloseOutcome";

import type { Notice } from "../../lib/notices";
import { readTeamClosePlanMetadata } from "./teamClosePlanRead";
import type { TeamCloseResource } from "./teamCloseResources";

type TeamCloseTarget = Pick<TeamCloseResource, "id" | "name" | "members"> & { workspaceId: string };
type TeamClosePreview = TeamCloseTarget & (
  | { phase: "loading" }
  | { phase: "error"; error: string }
  | ({ phase: "ready" } & TeamCloseResource)
);

export function useTeamClosePreview(read = readTeamClosePlanMetadata) {
  const [pending, setPending] = useState<TeamClosePreview | null>(null);
  const active = useRef<AbortController | null>(null);
  useEffect(() => () => active.current?.abort(), []);
  const dismiss = useCallback(() => {
    active.current?.abort();
    active.current = null;
    setPending(null);
  }, []);
  const request = useCallback(async (target: TeamCloseTarget) => {
    active.current?.abort();
    const controller = new AbortController();
    active.current = controller;
    setPending({ ...target, phase: "loading" });
    try {
      const { teams: [resources] } = await read(target.workspaceId, target.id, controller.signal);
      if (!controller.signal.aborted) setPending({ ...target, ...resources, phase: "ready" });
    } catch (error) {
      if (!controller.signal.aborted) {
        setPending({ ...target, phase: "error", error: error instanceof Error ? error.message : String(error) });
      }
    } finally {
      if (active.current === controller) active.current = null;
    }
  }, [read]);
  return { pending, request, dismiss };
}

export function useTeamClose(close: ((teamId: string) => Promise<TeamCloseInventory>) | undefined, notify: (notice: Notice) => void) {
  const [closingTeamIds, setClosingTeamIds] = useState<ReadonlySet<string>>(new Set());
  const inFlight = useRef(new Set<string>());
  const run = useCallback(async function run(teamId: string, workspaceId: string, dismiss: () => void) {
    if (!close || inFlight.current.has(teamId)) return;
    inFlight.current.add(teamId);
    setClosingTeamIds(ids => new Set([...ids, teamId]));
    let inventory: TeamCloseInventory = { removed: [], kept: [], orphan_branches: [] };
    let survivors: string[] = [];
    let verificationUnavailable = false;
    let closeFailure: unknown;
    let closeFailed = false;
    try {
      inventory = await startBackgroundClose(dismiss, () => close(teamId));
    } catch (error) {
      closeFailure = error;
      closeFailed = true;
      const [stateRead, cronRead, blueprintRead] = await Promise.allSettled([
        invoke<MultiplexerState>("mux_get_state"),
        invoke<CronJob[]>("cron_list", {}),
        readWorkspaceBlueprints(workspaceId),
      ]);
      const state = stateRead.status === "fulfilled" ? stateRead.value : null;
      const crons = cronRead.status === "fulfilled" ? cronRead.value : [];
      const blueprints = blueprintRead.status === "fulfilled" ? blueprintRead.value : [];
      survivors = teamCloseSurvivors(teamId, Object.values(state?.tabs ?? {}), crons, blueprints);
      if (state?.teams?.some(team => team.id === teamId) && survivors.length === 0) {
        survivors.push("Team is still open");
      }
      verificationUnavailable = [stateRead, cronRead, blueprintRead].some(read => read.status === "rejected");
    } finally {
      inFlight.current.delete(teamId);
      setClosingTeamIds(ids => new Set([...ids].filter(id => id !== teamId)));
    }
    const outcome = teamCloseOutcome(inventory, survivors);
    if (outcome.kind === "closed" && !verificationUnavailable && !closeFailed) return;
    const retry = outcome.kind === "partial" || verificationUnavailable;
    const title = outcome.kind === "partial" ? "Still open"
      : verificationUnavailable ? "Close status unavailable"
      : closeFailed ? "Team cleanup reported an error"
      : "Lanes preserved";
    notify({
      kind: "toast", tabId: null, workspaceId,
      title,
      detail: [verificationUnavailable && outcome.kind !== "partial" ? "The close could not be verified. Retry to finish closing." : (outcome.kind === "partial" ? outcome.survivors : outcome.preserved).join("\n"), closeFailure instanceof Error ? closeFailure.message : closeFailure == null ? "" : String(closeFailure)].filter(Boolean).join("\n"),
      severity: retry || closeFailed ? "Error" : "Info",
      ...(retry ? { actions: [{ label: "Retry", run: () => run(teamId, workspaceId, () => { }) }] } : {}),
    });
  }, [close, notify]);
  return { closingTeamIds, closeTeam: run };
}
