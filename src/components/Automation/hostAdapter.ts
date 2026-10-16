import { createPageLoadCache } from "../../hooks/pageLoadCache";
import { assertWebRequestContext, webRequestContext } from "../../lib/webApi";
import { orchestrationTransport, listenOrchestration as listen } from "../../lib/transport/orchestrationRuntime";
import { ALL_JOBS_SCOPE, sameScope, type AutomationGraphAdapter, type AutomationIdentity, type AutomationScope } from "./types";

export const AUTOMATION_GRAPH_EVENT = "automation-graph-changed";

const graphCache = createPageLoadCache(async (attachment: AutomationScope) => attachment.team_id === ALL_JOBS_SCOPE
  ? orchestrationTransport().automation_graph_workspace_snapshot({ workspaceId: attachment.workspace_id })
  : orchestrationTransport().automation_graph_snapshot({ workspaceId: attachment.workspace_id, teamId: attachment.team_id }));
let pageGeneration = -1;
let pageCacheActive = false;

const graphRevisions = new Map<string, string>();
function invalidateGraph(payload: AutomationScope & { revision?: string }) {
  const key = JSON.stringify([payload.workspace_id, payload.team_id]);
  if (payload.revision && graphRevisions.get(key) === payload.revision) return;
  if (payload.revision) graphRevisions.set(key, payload.revision);
  void graphCache.refresh((scope) => scope.workspace_id === payload.workspace_id
    && (scope.team_id === ALL_JOBS_SCOPE || sameScope(scope, payload))).catch(() => undefined);
}

export function startPageGraphCache() {
  pageCacheActive = true;
  let active = true;
  let off: (() => void) | undefined;
  let binding = 0;
  const bind = () => {
    const current = ++binding;
    off?.();
    void listen<AutomationScope & { revision: string }>(AUTOMATION_GRAPH_EVENT, ({ payload }) => {
      if (active && current === binding && payload) invalidateGraph(payload);
    }).then((stop) => { if (active && current === binding) off = stop; else stop(); }).catch(() => undefined);
  };
  bind();
  const reset = () => { graphCache.clear(); graphRevisions.clear(); };
  window.addEventListener("codeterm-peer-switch-reset", reset);
  window.addEventListener("codeterm-peer-switch-rebind", bind);
  return () => {
    active = false;
    pageCacheActive = false;
    off?.();
    graphCache.clear();
    graphRevisions.clear();
    window.removeEventListener("codeterm-peer-switch-reset", reset);
    window.removeEventListener("codeterm-peer-switch-rebind", bind);
  };
}

export const hostAutomationAdapter: AutomationGraphAdapter = {
  async read(attachment, signal) {
    signal.throwIfAborted();
    const generation = webRequestContext();
    if (generation !== pageGeneration) { graphCache.clear(); pageGeneration = generation; }
    const graph = pageCacheActive && attachment.connection_id === "sidebar"
      ? await graphCache.read({ workspace_id: attachment.workspace_id, team_id: attachment.team_id })
      : attachment.team_id === ALL_JOBS_SCOPE
        ? await orchestrationTransport().automation_graph_workspace_snapshot({ workspaceId: attachment.workspace_id })
        : await orchestrationTransport().automation_graph_snapshot({ workspaceId: attachment.workspace_id, teamId: attachment.team_id });
    signal.throwIfAborted();
    assertWebRequestContext(generation);
    return graph;
  },
  subscribe(attachment, changed) {
    return listen<AutomationScope & { revision: string }>(AUTOMATION_GRAPH_EVENT, ({ payload }) => {
      if (payload && payload.workspace_id === attachment.workspace_id && (attachment.team_id === ALL_JOBS_SCOPE || sameScope(payload, attachment))) {
        invalidateGraph(payload);
        changed();
      }
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
