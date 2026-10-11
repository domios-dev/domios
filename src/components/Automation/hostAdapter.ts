import { orchestrationTransport, listenOrchestration as listen } from "../../lib/transport/orchestrationRuntime";
import { ALL_JOBS_SCOPE, sameScope, type AutomationGraphAdapter, type AutomationIdentity, type AutomationScope } from "./types";

export const AUTOMATION_GRAPH_EVENT = "automation-graph-changed";

export const hostAutomationAdapter: AutomationGraphAdapter = {
  async read(attachment, signal) {
    signal.throwIfAborted();
    const graph = attachment.team_id === ALL_JOBS_SCOPE
      ? await orchestrationTransport().automation_graph_workspace_snapshot( { workspaceId: attachment.workspace_id })
      : await orchestrationTransport().automation_graph_snapshot( { workspaceId: attachment.workspace_id, teamId: attachment.team_id });
    signal.throwIfAborted();
    return graph;
  },
  subscribe(attachment, changed) {
    return listen<AutomationScope & { revision: string }>(AUTOMATION_GRAPH_EVENT, ({ payload }) => {
      if (payload && payload.workspace_id === attachment.workspace_id && (attachment.team_id === ALL_JOBS_SCOPE || sameScope(payload, attachment))) changed();
    });
  },
  async stop(attachment, identity) {
    if (!matchesActionScope(attachment, identity) || identity.kind !== "blueprint") throw new Error("Invalid Blueprint scope");
    await orchestrationTransport().automation_graph_stop_blueprint( {
      workspaceId: attachment.workspace_id,
      teamId: identity.team_id,
      id: identity.id,
    });
  },
  async delete(attachment, identity) {
    if (!matchesActionScope(attachment, identity) || identity.kind !== "blueprint") throw new Error("Invalid Blueprint scope");
    await orchestrationTransport().automation_graph_delete_blueprint( { workspaceId: attachment.workspace_id, teamId: identity.team_id, id: identity.id });
  },
};

function matchesActionScope(attachment: AutomationScope, identity: AutomationIdentity): boolean {
  return attachment.workspace_id === identity.workspace_id
    && (attachment.team_id === ALL_JOBS_SCOPE || attachment.team_id === identity.team_id);
}
