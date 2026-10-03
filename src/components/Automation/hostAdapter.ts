import { invoke, listen } from "../../lib/bridge";
import { sameScope, type AutomationGraph, type AutomationGraphAdapter, type AutomationScope } from "./types";

export const AUTOMATION_GRAPH_EVENT = "automation-graph-changed";

export const hostAutomationAdapter: AutomationGraphAdapter = {
  async read(attachment, signal) {
    signal.throwIfAborted();
    const graph = await invoke<AutomationGraph>("automation_graph_snapshot", {
      workspaceId: attachment.workspace_id,
      teamId: attachment.team_id,
    });
    signal.throwIfAborted();
    return graph;
  },
  subscribe(attachment, changed) {
    return listen<AutomationScope & { revision: string }>(AUTOMATION_GRAPH_EVENT, ({ payload }) => {
      if (payload && sameScope(payload, attachment)) changed();
    });
  },
  async stop(attachment, identity) {
    if (!sameScope(attachment, identity) || identity.kind !== "sentinel") throw new Error("Invalid Sentinel scope");
    await invoke("automation_graph_stop_sentinel", {
      workspaceId: attachment.workspace_id,
      teamId: attachment.team_id,
      id: identity.id,
    });
  },
};
