// Run: npx tsx src/components/Sidebar/SidebarAgentChecklist.test.tsx

import type { Tab } from "../../lib/types";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { remoteTransportCall } from "../../lib/remoteTabTransport";
import { boundAgent, tab, TEAM } from "./SidebarWorkspaceTabs.fixtures.testdata";
import { act, assert, createSuite, renderSidebar, setupDom } from "./SidebarWorkspaceTabs.testkit";

const { test, run } = createSuite("SidebarAgentChecklist");

setupDom();

function checklistTeam(): Record<string, Tab> {
  return {
    orchestrator: tab({ id: "orchestrator", title: "Orchestrator", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id, ...boundAgent("orchestrator") }),
    worker: tab({ id: "worker", title: "Mica", role: "Worker", team_id: TEAM.id, parent_tab_id: "orchestrator", has_checklist: true, ...boundAgent("worker") }),
  };
}

function checklistRecord(tabId: string, generation: number, revision: number, items: Array<[string, string, string?]>) {
  return {
    tab_id: tabId, generation, revision, started_at: 1, status: "active",
    items: items.map(([title, state, reason], position) => ({ id: `${generation}-${position}`, position, title, state, reason: reason ?? null, revision: 1 })),
  };
}

function checklistStore(initial: unknown) {
  let current = initial;
  const reads: unknown[] = [];
  const handlers = new Set<(event: { payload: unknown }) => void>();
  __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
    if (command !== "tab_todo_list") return {};
    reads.push(args?.tabId);
    return structuredClone(current);
  }) as never);
  __setListenForTest((async (event: string, handler: (event: { payload: unknown }) => void) => {
    if (event !== "tab-todo-changed") return () => {};
    handlers.add(handler);
    return () => handlers.delete(handler);
  }) as never);
  return {
    reads,
    readCount: () => reads.length,
    handlers,
    set: (next: unknown) => { current = next; },
    emit: async (payload: unknown) => {
      await act(async () => {
        for (const handler of [...handlers]) handler({ payload });
        await Promise.resolve();
      });
    },
    restore: () => {
      __setInvokeForTest(null);
      __setListenForTest(async () => () => {});
    },
  };
}

function checklistRows(region: Element): string[] {
  return [...region.querySelectorAll("li")].map((item) => item.textContent ?? "");
}

function toggleFor(host: HTMLElement, title: string): HTMLButtonElement | null {
  return host.querySelector(`button[aria-label="Checklist for ${title}"]`);
}

function regionFor(host: HTMLElement, title: string): Element | null {
  return host.querySelector(`[role="region"][aria-label="Checklist of ${title}"]`);
}

async function open(toggle: HTMLButtonElement) {
  await act(async () => toggle.click());
  await act(async () => { await Promise.resolve(); });
}

test("a checklist disclosure reads the store only when opened and shows titles with states", async () => {
  const store = checklistStore(checklistRecord("worker", 1, 3, [
    ["Ship the route", "done"], ["Wire the row", "in_progress"], ["Record evidence", "open"],
    ["Wait for the API", "blocked", "route owner undecided"], ["Polish copy", "skipped", "out of scope"],
  ]));
  const { host, root } = await renderSidebar({ tabs: checklistTeam(), sortedTabs: [tab("o", "orchestrator", 0), tab("w", "worker", 1)] });
  const toggle = toggleFor(host, "Mica");
  assert(toggle, "a row whose tab holds a checklist exposes a named disclosure");
  assert(toggle.getAttribute("aria-expanded") === "false", "disclosure starts collapsed");
  assert(!toggleFor(host, "Orchestrator"), "a row without a checklist carries no disclosure");
  assert(store.reads.length === 0 && store.handlers.size === 0, "collapsed rows neither fetch nor subscribe");

  await open(toggle);
  const region = regionFor(host, "Mica");
  assert(region, "opening reveals the checklist region");
  assert(toggle.getAttribute("aria-expanded") === "true" && toggle.getAttribute("aria-controls") === region.id, "disclosure controls the region");
  assert(JSON.stringify(store.reads) === JSON.stringify(["worker"]), "opening reads the tab's checklist once");
  assert(
    JSON.stringify(checklistRows(region)) === JSON.stringify([
      "Ship the routeDone", "Wire the rowIn progress", "Record evidenceOpen", "Wait for the APIBlocked", "Polish copySkipped",
    ]),
    "region lists every item title with its current state in order, blocked and skipped included",
  );
  const blocked = [...region.querySelectorAll("li")].find((item) => item.textContent?.startsWith("Wait for the API"));
  assert(blocked?.querySelector("[title]")?.getAttribute("title") === "Wait for the API — route owner undecided", "a blocked item carries its reason");

  await act(async () => toggle.click());
  assert(!regionFor(host, "Mica"), "collapsing hides the detail");
  assert(store.handlers.size === 0, "collapsing drops the change subscription");

  store.set(null);
  await open(toggle);
  assert(regionFor(host, "Mica")?.textContent === "No checklist", "a vanished checklist shows the empty state");
  const stored = Object.keys(window.localStorage).map((key) => window.localStorage.getItem(key) ?? "").join("\n");
  assert(!stored.includes("Ship the route"), "checklist rows never reach localStorage");
  await act(async () => root.unmount());
  store.restore();
});

test("an Orchestrator row shows the checklist it wrote for itself", async () => {
  const store = checklistStore(checklistRecord("orchestrator", 1, 2, [["Merge wave one", "in_progress"], ["Release", "open"]]));
  const tabs = checklistTeam();
  tabs.orchestrator = { ...tabs.orchestrator, has_checklist: true };
  const { host, root } = await renderSidebar({ tabs, sortedTabs: [tab("o", "orchestrator", 0), tab("w", "worker", 1)] });
  const toggle = toggleFor(host, "Orchestrator");
  assert(toggle, "the Orchestrator row exposes a checklist disclosure");
  await open(toggle);
  const region = regionFor(host, "Orchestrator");
  assert(region, "the Orchestrator checklist region opens");
  assert(JSON.stringify(store.reads) === JSON.stringify(["orchestrator"]), "the Orchestrator's own checklist is read");
  assert(JSON.stringify(checklistRows(region)) === JSON.stringify(["Merge wave oneIn progress", "ReleaseOpen"]), "its items and states are shown");
  await act(async () => root.unmount());
  store.restore();
});

test("roleless agents and remote peer rows show a reported checklist; plain shells never do", async () => {
  const store = checklistStore(checklistRecord("solo", 1, 1, [["Own step", "open"]]));
  const tabs: Record<string, Tab> = {
    solo: tab({ id: "solo", title: "Solo", has_checklist: true, ...boundAgent("solo") }),
    "mesh:peer-1/remote-agent": tab({ id: "mesh:peer-1/remote-agent", title: "Remote", remote_peer_id: "peer-1", has_checklist: true, ...boundAgent("remote") }),
    shell: tab({ id: "shell", title: "Shell", has_checklist: true }),
  };
  const { host, root } = await renderSidebar({ tabs });
  assert(toggleFor(host, "Solo"), "a roleless agent row carries the disclosure");
  assert(!toggleFor(host, "Shell"), "a plain shell row never carries the disclosure");
  const remote = toggleFor(host, "Remote");
  assert(remote, "a remote peer row that reports a checklist carries the disclosure");
  await open(remote);
  assert(JSON.stringify(store.reads) === JSON.stringify(["mesh:peer-1/remote-agent"]), "the remote row reads through its own tab id");
  await act(async () => root.unmount());
  store.restore();
});

test("a remote checklist read relays to the peer's own todo route", async () => {
  const call = remoteTransportCall("tab_todo_list", { tabId: "mesh:peer-1/remote-agent" });
  assert(call?.method === "GET" && call.suffix === "/todos" && call.remoteTabId === "remote-agent", "remote read targets GET /todos on the peer");
});

test("a row keeps its DOM node when its checklist appears or clears", async () => {
  const store = checklistStore(checklistRecord("worker", 1, 1, [["Step", "open"]]));
  const withChecklist = (present: boolean): Record<string, Tab> => {
    const tabs = checklistTeam();
    return {
      ...tabs,
      worker: { ...tabs.worker, has_checklist: present },
      solo: tab({ id: "solo", title: "Solo", has_checklist: present, ...boundAgent("solo") }),
    };
  };
  const sortedTabs = ["orchestrator", "worker", "solo"];
  const { host, root, rerenderTabs } = await renderSidebar({ tabs: withChecklist(false), sortedTabs });
  const rowNode = (id: string) => host.querySelector(`[data-tab-id="${id}"]`);
  const before = { worker: rowNode("worker"), solo: rowNode("solo") };
  assert(before.worker && before.solo && !toggleFor(host, "Mica"), "rows render without a checklist toggle");
  for (const present of [true, false]) {
    await rerenderTabs(withChecklist(present));
    assert(Boolean(toggleFor(host, "Mica")) === present && Boolean(toggleFor(host, "Solo")) === present, `toggle follows has_checklist=${present}`);
    assert(rowNode("worker") === before.worker, "the team member row is not remounted");
    assert(rowNode("solo") === before.solo, "the loose agent row is not remounted");
  }
  await act(async () => root.unmount());
  store.restore();
});

test("a checklist rereads on its own change event and never shows a stale generation", async () => {
  const store = checklistStore(checklistRecord("worker", 1, 2, [["Old step", "in_progress"]]));
  const { host, root } = await renderSidebar({ tabs: checklistTeam(), sortedTabs: [tab("o", "orchestrator", 0), tab("w", "worker", 1)] });
  const toggle = toggleFor(host, "Mica")!;
  await open(toggle);
  const region = () => regionFor(host, "Mica")!;
  assert(JSON.stringify(checklistRows(region())) === JSON.stringify(["Old stepIn progress"]), "initial read is shown");

  store.set(checklistRecord("worker", 2, 1, [["Fresh step", "open"], ["Second step", "done"]]));
  await store.emit({ tab_id: "other-worker", generation: 9, revision: 9 });
  await store.emit({ tab_id: "worker", generation: 1, revision: 2 });
  assert(store.readCount() === 1, "foreign and already-shown events do not reread");
  assert(JSON.stringify(checklistRows(region())) === JSON.stringify(["Old stepIn progress"]), "detail is unchanged until a matching event");

  await store.emit({ tab_id: "worker", generation: 2, revision: 1 });
  assert(store.readCount() === 2, "a newer generation event rereads the authorized list once");
  assert(
    JSON.stringify(checklistRows(region())) === JSON.stringify(["Fresh stepOpen", "Second stepDone"]),
    "reset generation replaces the rows and the stale rows disappear without reload",
  );

  store.set({ ...checklistRecord("worker", 2, 2, []), status: "cleared" });
  await store.emit({ tab_id: "worker", generation: 2, revision: 2 });
  assert(region().textContent === "No checklist", "a cleared generation leaves no stale rows");

  await act(async () => toggle.click());
  await store.emit({ tab_id: "worker", generation: 3, revision: 1 });
  assert(store.readCount() === 3, "a collapsed disclosure ignores later events");
  await act(async () => root.unmount());
  store.restore();
});

void run();
