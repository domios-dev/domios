import { createPageLoadCache } from "../../hooks/pageLoadCache";
import { DEV_BUILD } from "../../lib/devBuild";
import { assertWebRequestContext, webRequestContext } from "../../lib/webApi";
import { orchestrationTransport, listenOrchestration as listen } from "../../lib/transport/orchestrationRuntime";
import { ALL_JOBS_SCOPE, sameScope, type AutomationGraph, type AutomationGraphAdapter, type AutomationIdentity, type AutomationScope } from "./types";

export const AUTOMATION_GRAPH_EVENT = "automation-graph-changed";
export const BLUEPRINT_CHAT_CONNECTION = "blueprint-chat";
/** Surfaces that remount with tabs share one event-refreshed read per scope; the Jobs view reads fresh. */
const PAGE_CACHED_CONNECTIONS = new Set(["sidebar", "composer", BLUEPRINT_CHAT_CONNECTION]);

const lastKnownGraphs = new Map<string, AutomationGraph>();
const scopeKey = (scope: AutomationScope) => JSON.stringify([scope.workspace_id, scope.team_id]);

const graphCache = createPageLoadCache(async (attachment: AutomationScope) => {
  const generation = webRequestContext();
  const graph = await (attachment.team_id === ALL_JOBS_SCOPE
    ? orchestrationTransport().automation_graph_workspace_snapshot({ workspaceId: attachment.workspace_id })
    : orchestrationTransport().automation_graph_snapshot({ workspaceId: attachment.workspace_id, teamId: attachment.team_id }));
  if (generation === webRequestContext()) lastKnownGraphs.set(scopeKey(attachment), graph);
  return graph;
});
let pageGeneration = -1;
let pageCacheActive = false;

function clearPageGraphs() {
  graphCache.clear();
  lastKnownGraphs.clear();
}

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
  const reset = () => { clearPageGraphs(); graphRevisions.clear(); };
  window.addEventListener("codeterm-peer-switch-reset", reset);
  window.addEventListener("codeterm-peer-switch-rebind", bind);
  return () => {
    active = false;
    pageCacheActive = false;
    off?.();
    clearPageGraphs();
    graphRevisions.clear();
    window.removeEventListener("codeterm-peer-switch-reset", reset);
    window.removeEventListener("codeterm-peer-switch-rebind", bind);
  };
}

export const hostAutomationAdapter: AutomationGraphAdapter = {
  async read(attachment, signal) {
    signal.throwIfAborted();
    const generation = webRequestContext();
    if (generation !== pageGeneration) { clearPageGraphs(); pageGeneration = generation; }
    const graph = pageCacheActive && PAGE_CACHED_CONNECTIONS.has(attachment.connection_id)
      ? await graphCache.read({ workspace_id: attachment.workspace_id, team_id: attachment.team_id })
      : attachment.team_id === ALL_JOBS_SCOPE
        ? await orchestrationTransport().automation_graph_workspace_snapshot({ workspaceId: attachment.workspace_id })
        : await orchestrationTransport().automation_graph_snapshot({ workspaceId: attachment.workspace_id, teamId: attachment.team_id });
    signal.throwIfAborted();
    assertWebRequestContext(generation);
    return graph;
  },
  peek(attachment) {
    if (!pageCacheActive || !PAGE_CACHED_CONNECTIONS.has(attachment.connection_id) || webRequestContext() !== pageGeneration) return null;
    const exact = lastKnownGraphs.get(scopeKey(attachment));
    if (exact || attachment.team_id === ALL_JOBS_SCOPE) return exact ?? null;
    const workspace = lastKnownGraphs.get(scopeKey({ workspace_id: attachment.workspace_id, team_id: ALL_JOBS_SCOPE }));
    return workspace ? { ...workspace, team_id: attachment.team_id, all_teams: false } : null;
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

/** Reads each workspace's all-Teams graph before it is opened, one at a time, so its first switch has nothing left to arrive. */
export async function primeWorkspaceGraphs(workspaceIds: readonly string[], signal: AbortSignal) {
  if (!pageCacheActive) return;
  for (const workspace_id of workspaceIds) {
    if (signal.aborted) return;
    try { await hostAutomationAdapter.read({ connection_id: "sidebar", workspace_id, team_id: ALL_JOBS_SCOPE }, signal); }
    catch (error) { if (DEV_BUILD) console.debug("Background workspace graph priming failed", error); }
  }
}

function matchesActionScope(attachment: AutomationScope, identity: AutomationIdentity): boolean {
  return attachment.workspace_id === identity.workspace_id
    && (attachment.team_id === ALL_JOBS_SCOPE || attachment.team_id === identity.team_id);
}
