import type { Notification } from "../../lib/types";
import type { BlueprintRunPopup, GraphRun } from "../../lib/blueprintRun";
import { graphRuns } from "./blueprintRunModel";
import type { AutomationGraph } from "./types";

export interface PopupState {
  ready: boolean;
  seen: Map<string, { signature: string; state: GraphRun["state"] }>;
  recent: Map<string, { shownAt: number; count: number; runId: string }>;
}

export const emptyPopupState = (): PopupState => ({ ready: false, seen: new Map(), recent: new Map() });

export function runPopupTitle(data: BlueprintRunPopup) {
  const event = data.run.events[data.run.events.length - 1];
  const outcome = data.run.state === "running" || data.run.state === "unconfirmed" ? data.run.state : data.run.events.some((entry) => entry.status === "failed") ? "failed" : event?.outcome ?? "completed";
  return `Blueprint ${data.run.name} ran: ${outcome}${data.count > 1 ? ` (${data.count} runs)` : ""}`;
}

export function advanceRunPopups(previous: PopupState, graph: AutomationGraph, now: number, visible: ReadonlySet<string>) {
  const next: PopupState = { ready: true, seen: new Map(), recent: new Map(previous.recent) };
  const toasts = new Map<string, Omit<Notification, "timestamp" | "read">>();
  const groupedRuns = new Map(graph.runs?.map((run) => [run.id, run]) ?? []);
  const scenarios = new Map(graph.scenarios.map((scenario) => [scenario.root.id, scenario]));
  for (const run of graphRuns(graph).reverse()) {
    if (run.state === "unlinked") continue;
    const signature = JSON.stringify([run.state, run.events, run.incomplete]);
    next.seen.set(run.key, { signature, state: run.state });
    if (!previous.ready || previous.seen.get(run.key)?.signature === signature) continue;
    const grouped = groupedRuns.get(run.key);
    const scenario = grouped ? scenarios.get(grouped.root_node) : undefined;
    if (!scenario) continue;
    const id = JSON.stringify(["blueprint-run", graph.workspace_id, scenario.root.team_id, scenario.id]);
    const recent = next.recent.get(id);
    const windowReady = !recent || now - recent.shownAt >= 60_000;
    const finishing = previous.seen.get(run.key)?.state === "running" && run.state !== "running" && recent?.runId === run.key;
    const show = windowReady || finishing;
    const count = windowReady ? 1 : Math.max(1, (recent?.count ?? 0) + (previous.seen.has(run.key) ? 0 : 1));
    const emitting = show || visible.has(id) || toasts.has(id);
    next.recent.set(id, { shownAt: windowReady ? now : recent.shownAt, count, runId: emitting ? run.key : recent.runId });
    if (!emitting) continue;
    const data: BlueprintRunPopup = { workspaceId: graph.workspace_id, teamId: scenario.root.team_id, scenarioId: scenario.id, run, count };
    toasts.set(id, { id, key: id, title: runPopupTitle(data), body: null, workspace_id: graph.workspace_id, tab_id: null, severity: "Info", level: "verbose", blueprint_run: data });
  }
  while (next.recent.size > 256) {
    const oldest = next.recent.keys().next().value;
    if (oldest === undefined) break;
    next.recent.delete(oldest);
  }
  return { state: next, toasts: [...toasts.values()] };
}
