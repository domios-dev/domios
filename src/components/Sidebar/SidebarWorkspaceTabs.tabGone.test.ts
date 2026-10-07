import type { Tab, TabStatus } from "../../lib/types";
import { ORCHESTRATOR_KIND } from "../../lib/roleKind";
import { groupWorkspaceTeamMembers, visibleWorkspaceTabs } from "./workspaceTeamModel";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function tab(id: string, role: Tab["role"], roleProfile: string | null, parent?: string): Tab {
  return {
    id,
    pty_id: id,
    tab_type: "Terminal",
    title: id,
    cwd: null,
    role,
    team_id: "team",
    role_profile: roleProfile,
    parent_tab_id: parent ?? null,
  } as Tab;
}

function collectIds(tabs: Record<string, Tab>): string[] {
  return visibleWorkspaceTabs(Object.values(tabs), tabs).map((tab) => tab.id);
}

function workerIds(tabs: Record<string, Tab>): string[] {
  const visible = visibleWorkspaceTabs(Object.values(tabs), tabs);
  return (groupWorkspaceTeamMembers(visible).get("team") ?? []).map((member) => member.tab.id);
}

const orchestrator = tab("orch", "Manager", ORCHESTRATOR_KIND);
const liveWorker = tab("worker-live", "Worker", null, "orch");
const goneWorker = tab("worker-gone", "Worker", null, "orch");
const statuses: Record<string, TabStatus> = {
  orch: "Idle",
  "worker-live": "Working",
  "worker-gone": "Working",
};

{
  const tabs = { orch: orchestrator, "worker-live": liveWorker, "worker-gone": goneWorker };
  const ids = workerIds(tabs);
  assert(ids.includes("worker-live"), "live worker present pre-close");
  assert(ids.includes("worker-gone"), "gone worker present pre-close");
}

{
  const tabs = { orch: orchestrator, "worker-live": liveWorker };
  const ids = workerIds(tabs);
  assert(ids.includes("worker-live"), "live worker still rendered");
  assert(!ids.includes("worker-gone"), "closed worker must NOT render as a live child");

  assert(statuses["worker-gone"] === "Working", "stale status retained");
  const workspaceIds = collectIds(tabs);
  assert(workspaceIds.includes("orch"), "orchestrator root present");
  assert(!workspaceIds.includes("worker-gone"), "workspace rows must not render gone worker");
}

console.log("✓ workspace sidebar: closed worker Tab is not rendered as a live child");

const staleOrder = [orchestrator, liveWorker, goneWorker];
const current = { orch: orchestrator, "worker-live": liveWorker };
assert(!visibleWorkspaceTabs(staleOrder, current).some((tab) => tab.id === "worker-gone"), "stale workspace ordering cannot resurrect a tab");

const freshWorker = { ...liveWorker, title: "Fresh worker" };
const aliasedSnapshot = { leader: orchestrator, worker: freshWorker };
const visibleAliased = visibleWorkspaceTabs(staleOrder, aliasedSnapshot);
assert(visibleAliased.map((tab) => tab.id).join(",") === "orch,worker-live", "live membership uses snapshot IDs despite record aliases");
assert(visibleAliased[1] === freshWorker, "ordered rows use the current snapshot object");
