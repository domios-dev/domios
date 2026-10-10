import { act, assert, createSuite, renderSidebar, setupDom } from "./SidebarWorkspaceTabs.testkit";
import { plan, planIndexEntry, tab, task, TEAM } from "./SidebarWorkspaceTabs.fixtures.testdata";

setupDom();
const { test, run } = createSuite("workspace team membership");

test("TeamUnit rows follow the live snapshot despite stale task and status references", async () => {
  const orch = tab({ id: "orch", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id });
  const live = tab({ id: "worker-live", role: "Worker", parent_tab_id: "orch", team_id: TEAM.id });
  const gone = tab({ id: "worker-gone", role: "Worker", parent_tab_id: "orch", team_id: TEAM.id });
  const staleTask = { ...task("assigned", "in_progress"), assigned_tab_id: gone.id };
  const index = { ...planIndexEntry(), assigned_tab_ids: [orch.id, live.id, gone.id] };
  const { host, root, rerenderTabs } = await renderSidebar({
    tabs: { [orch.id]: orch, [live.id]: live, [gone.id]: gone },
    plans: [plan([staleTask])],
    planIndex: [index],
    statuses: { [live.id]: "Working", [gone.id]: "Working" },
  });
  try {
    assert(host.querySelector('[data-tab-id="worker-live"]'), "live worker wired to a row");
    assert(host.querySelector('[data-tab-id="worker-gone"]'), "second worker wired before close");
    await rerenderTabs({ [orch.id]: orch, [live.id]: live });
    assert(host.querySelector('[data-tab-id="orch"]'), "orchestrator row retained");
    assert(host.querySelector('[data-tab-id="worker-live"]'), "live worker row retained");
    assert(!host.querySelector('[data-tab-id="worker-gone"]'), "stale task and status cannot create a row");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

void run();
