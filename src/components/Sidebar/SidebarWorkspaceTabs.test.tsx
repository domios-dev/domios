// Run: npx tsx src/components/Sidebar/SidebarWorkspaceTabs.test.tsx

import type { ProviderDescriptor, Tab, TabStatus, Workspace } from "../../lib/types";
import { TaskStatus, type Plan } from "../Canvas/types";
import { PLAN_STATUS_COLORS } from "../shared/SegmentedDonut";
import { canvasFocusStore } from "../Canvas/canvasFocusStore";
import { SidebarDnd } from "./sortable";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";
import { forwardTerminalFallback } from "../../lib/terminalFallbackInput";
import { __setInvokeForTest } from "../../lib/bridge";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import {
  plan,
  planIndexEntry,
  tab,
  task,
  TEAM,
} from "./SidebarWorkspaceTabs.fixtures.testdata";
import {
  act,
  assert,
  createRoot,
  createSuite,
  openTabActionsMenu,
  renderSidebar,
  setupDom,
  sidebarStyles,
  useState,
} from "./SidebarWorkspaceTabs.testkit";

const { test, run } = createSuite("SidebarWorkspaceTabs");

setupDom();

test("new session stays disabled until Get started is complete", async () => {
  const { host, root } = await renderSidebar({
    tabs: {},
    sortedTabs: [],
    newSessionDisabled: true,
  });
  const button = host.querySelector('[data-testid="sidebar-new-session"]') as HTMLButtonElement;
  assert(button.disabled, "first-run General workspace must disable New session");
  assert(
    button.title === "Finish Get started before creating another session",
    "disabled action explains how to unlock it",
  );
  await act(async () => root.unmount());
});

test("hidden service Tabs never render in the sidebar", async () => {
  const visibleTab = tab({ id: "visible-tab", title: "Get started" });
  const hiddenTab = { ...tab({ id: "probe-tab", title: "Puccini" }), hidden: true };
  const unpublishedTab = {
    ...tab({ id: "unpublished-tab", title: "Rossini" }),
    show_in_tabbar: false,
  };
  const { host, root } = await renderSidebar({
    tabs: {
      [visibleTab.id]: visibleTab,
      [hiddenTab.id]: hiddenTab,
      [unpublishedTab.id]: unpublishedTab,
    },
    sortedTabs: [
      tab("visible-tab", visibleTab.id, 0),
      tab("probe-tab", hiddenTab.id, 1),
      tab("unpublished-tab", unpublishedTab.id, 2),
    ],
  });
  assert(host.textContent?.includes("Get started"), "visible onboarding Tab remains rendered");
  assert(!host.textContent?.includes("Puccini"), "hidden usage probe must not leak into the sidebar");
  assert(!host.textContent?.includes("Rossini"), "unpublished service Tab must not leak into the sidebar");
  await act(async () => root.unmount());
});

test("team_renders_flat_sections", async () => {
  const tabs: Record<string, Tab> = {
    plannerTwo: tab({ id: "plannerTwo", title: "Planner Two", role: "Manager", role_profile: "planner", team_id: TEAM.id }),
    workerOne: tab({ id: "workerOne", title: "Worker One", role: "Worker", team_id: TEAM.id, parent_tab_id: "orchOne" }),
    orchTwo: tab({ id: "orchTwo", title: "Orchestrator Two", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
    plannerOne: tab({ id: "plannerOne", title: "Planner One", role: "Manager", role_profile: "planner", team_id: TEAM.id }),
    workerTwo: tab({ id: "workerTwo", title: "Worker Two", role: "Worker", team_id: TEAM.id, parent_tab_id: "workerOne" }),
    orchOne: tab({ id: "orchOne", title: "Orchestrator One", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
    workerThree: tab({ id: "workerThree", title: "Worker Three", role: "Worker", team_id: TEAM.id, parent_tab_id: "orchTwo" }),
  };
  const ids = Object.keys(tabs);
  const sortedTabs = ids.map((id, index) => tab(`tab-${id}`, id, index));
  const { host, root } = await renderSidebar({ tabs, sortedTabs });

  const team = host.querySelector('[data-testid="team-unit"]');
  const managers = team?.querySelector('[data-testid="managers-section"]');
  const workers = team?.querySelector('[data-testid="workers-section"]');
  assert(team && managers && workers, "team unit renders MANAGERS and collapsible WORKERS");
  const managerTitles = Array.from(managers.querySelectorAll('[data-testid="tab-title"]')).map(
    (element) => element.textContent,
  );
  assert(
    managerTitles.join(",") === "Orchestrator Two,Orchestrator One,Planner Two,Planner One",
    `orchestrators sort before planners: ${managerTitles.join(",")}`,
  );
  assert(workers.querySelectorAll('[data-testid="tab-item"]').length === 3, "three workers render once");
  assert(!team.querySelector(".sidebar-hierarchy-level"), "no nested member tree nodes");
  await act(async () => root.unmount());
});

test("plans have only a Team-owned sidebar home", async () => {
  const tabs = {
    orch: tab({ id: "orch", title: "Orchestrator", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
  };
  const sortedTabs = [tab("tab-orch", "orch", 0)];
  const bound = plan([]);
  const detached = { ...bound, team_id: null } as Plan;
  const { host, root, rerender } = await renderSidebar({
    sortedTabs,
    tabs,
    plans: [],
    planIndex: [planIndexEntry()],
  });
  assert(host.querySelector('[data-testid="team-card-activation"]')?.getAttribute("aria-label") === `Open ${bound.title}`, "the plan index keeps the Team plan visible before its full body arrives");
  await rerender([bound], [planIndexEntry()]);
  assert(host.querySelector('[data-testid="team-card-activation"]')?.getAttribute("aria-label") === `Open ${bound.title}`, "bound plan is owned by its Team card");
  assert(!host.querySelector('[data-testid="workspace-plan-entry"]'), "bound plan has no standalone row");
  await rerender([detached], [planIndexEntry("plan-team", null)]);
  assert(!host.querySelector('[data-testid="team-card-activation"]'), "detach removes the plan from the Team card");
  assert(!host.querySelector('[data-testid="workspace-plan-entry"]'), "orphan plan has no workspace home");
  await rerender([bound], [planIndexEntry()]);
  assert(host.querySelector('[data-testid="team-card-activation"]')?.getAttribute("aria-label") === `Open ${bound.title}`, "adopt restores the Team-owned plan");
  assert(!host.querySelector('[data-testid="workspace-plan-entry"]'), "adopt removes the standalone row without reload");
  await act(async () => root.unmount());
});


test("teamless_tabs_render_plain", async () => {
  const tabs = {
    looseOne: tab({ id: "looseOne", title: "Loose terminal" }),
    looseTwo: tab({ id: "looseTwo", title: "Quick fix", role: "Worker" }),
  };
  const sortedTabs = [tab("tab-loose-one", "looseOne", 0), tab("tab-loose-two", "looseTwo", 1)];
  const { host, root } = await renderSidebar({ tabs, sortedTabs });

  assert(!host.querySelector('[data-testid="team-unit"]'), "teamless tabs have no team wrapper");
  assert(host.querySelectorAll('[data-testid="tab-item"]').length === 2, "both loose tabs render plainly");
  await act(async () => root.unmount());
});

test("team header renders the live compact status tally", async () => {
  const tabs = {
    orchestrator: tab({ id: "orchestrator", title: "Orchestrator", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
    worker: tab({ id: "worker", title: "Worker", role: "Worker", team_id: TEAM.id }),
  };
  const currentPlan = plan([
    task("pending-one", TaskStatus.Pending),
    task("pending-two", TaskStatus.Pending),
    task("progress", TaskStatus.InProgress),
    task("review", TaskStatus.InReview),
    task("blocked", TaskStatus.Blocked),
    task("done-one", TaskStatus.Done),
    task("done-two", TaskStatus.Done),
    task("done-three", TaskStatus.Done),
  ]);
  const { host, root, rerender } = await renderSidebar({
    tabs,
    sortedTabs: [tab("tab-orchestrator", "orchestrator", 0), tab("tab-worker", "worker", 1)],
    plans: [currentPlan],
    statuses: { worker: "Working" },
  });

  const tally = host.querySelector('[data-testid="team-status-tally"]');
  assert(host.querySelector('[data-testid="team-donut"]'), "bound team renders its status donut");
  assert(!host.querySelector('[data-testid="team-donut"] svg'), "donut has no inner icon node");
  assert(!host.querySelector('[data-testid="team-open-effort"]'), "superseded effort row is absent");
  assert(
    Array.from(tally?.querySelectorAll('[data-testid^="team-status-"]') ?? []).map((item) => item.textContent?.trim()).join(",") === "3 todo,1 review,1 blocked,3 done",
    `tally=${tally?.textContent}`,
  );
  const items = Array.from(tally?.querySelectorAll('[data-testid^="team-status-"]') ?? []) as HTMLElement[];
  assert(items.map((item) => item.style.color).join(",") === [
    PLAN_STATUS_COLORS.todo,
    PLAN_STATUS_COLORS.review,
    PLAN_STATUS_COLORS.blocked,
    PLAN_STATUS_COLORS.done,
  ].join(","), "tally colors use runtime theme variables in board order");
  assert(items.length === 4, "tally exposes the four compact status buckets");
  assert(items.map((item) => item.querySelector(".ct-plan-status-breakdown-label")?.textContent?.trim()).join(",") === "todo,review,blocked,done", "paused stays out of the compact tally");
  assert(tally?.textContent?.replace(/\s+/g, " ").trim() === "3 todo · 1 review · 1 blocked · 3 done", "tally uses middle-dot separators");
  const managers = host.querySelector('[data-testid="managers-section"]');
  const disclosure = host.querySelector('[data-testid="team-collapse-toggle"]');
  assert(
    Boolean(tally && disclosure && managers
      && (tally.compareDocumentPosition(managers) & Node.DOCUMENT_POSITION_FOLLOWING)
      && (managers.compareDocumentPosition(disclosure) & Node.DOCUMENT_POSITION_FOLLOWING)),
    "plan tally precedes Team members while disclosure stays below them",
  );
  assert(/\.sidebar-team-readout\s*\{[^}]*margin:\s*6px 12px 9px 14px;[^}]*justify-content:\s*flex-start/.test(sidebarStyles), "tally is aligned to the Team content edge");
  const card = host.querySelector('[data-testid="team-card-activation"]') as HTMLButtonElement | null;
  assert(card, "bound Team exposes a dedicated plan activation surface");
  assert(!host.querySelector('[data-testid="team-plan-button"]'), "superseded Plan button is absent");
  assert(!host.querySelector('[data-testid="team-effort"]'), "redundant plan title is not repeated below the Team title");
  assert(!host.querySelector('[data-testid="team-done-count"]'), "header does not show a done/total progress fraction");
  const title = host.querySelector('[data-testid="team-name"]');
  const working = host.querySelector('[data-testid="team-working-indicator"]');
  assert(title?.nextElementSibling === working, "working indicator follows the Team title");
  assert(working?.getAttribute("role") === "status", "working indicator is announced to assistive technology");
  assert(!host.textContent?.includes("Open effort"), "Team unit has no Open effort chrome");
  canvasFocusStore.closeWorkspace();
  await act(async () => card.click());
  assert(
    canvasFocusStore.getWorkspaceTarget()?.planId === currentPlan.id,
    "bound Team card opens the exact bound plan through the Team canvas entry",
  );

  await act(async () => (disclosure as HTMLButtonElement).click());
  assert(
    host.querySelector('[data-testid="team-status-tally"]'),
    "task status tally remains visible while the Team is collapsed",
  );

  const updatedPlan = plan(currentPlan.tasks.map((item) =>
    item.id === "blocked" ? { ...item, status: TaskStatus.Done } : item
  ));
  await rerender([updatedPlan]);
  assert(
    Array.from(host.querySelector('[data-testid="team-status-tally"]')?.querySelectorAll('[data-testid^="team-status-"]') ?? []).map((item) => item.textContent?.trim()).join(",") === "3 todo,1 review,0 blocked,4 done",
    "live plan updates replace the tally counts",
  );
  await act(async () => root.unmount());
});

test("team_card_activation_covers_title_tally_and_lower_area", async () => {
  const tabs = {
    orchestrator: tab({ id: "orchestrator", title: "Orchestrator", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
  };
  const currentPlan = plan([task("done", TaskStatus.Done)]);
  const activatedTabs: string[] = [];
  canvasFocusStore.closeWorkspace();
  const { host, root } = await renderSidebar({
    tabs,
    sortedTabs: [tab("tab-orchestrator", "orchestrator", 0)],
    plans: [currentPlan],
    onRenameTeam: () => {},
    onActivateTab: (tabId) => activatedTabs.push(tabId),
  });
  const card = host.querySelector('[data-testid="team-card-activation"]') as HTMLButtonElement;
  const chevron = host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement;
  const menu = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement;
  const header = host.querySelector('.sidebar-team-header') as HTMLElement;
  const sampledAreas = [
    host.querySelector('[data-testid="team-card-title-area"]'),
    host.querySelector('[data-testid="team-status-tally"]'),
    host.querySelector('[data-team-card-region="lower-card"]'),
  ];
  assert(card.tabIndex === 0 && chevron.tabIndex === 0 && menu.tabIndex === 0, "card, chevron, and menu are distinct keyboard stops");
  assert(!header.hasAttribute("role"), "header does not nest controls in a button role");
  assert(card.parentElement === host.querySelector('[data-testid="team-unit"]'), "card activation is a sibling of the Team visual regions");
  assert(!card.querySelector("button, input, select, textarea, a, [role=button], [tabindex]"), "full-card activation contains no nested controls");
  assert(card.nextElementSibling === header, "card activation sits beside the title header instead of wrapping it");
  assert(!card.contains(menu) && !card.contains(chevron), "menu and disclosure stay outside the full-card activation");
  assert(sampledAreas.every((area) => area?.getAttribute("data-team-card-hit-target") === card.dataset.testid), "title, tally, and lower card resolve to the one activation target");
  assert(/\.sidebar-team-unit-activatable \.sidebar-team-card-activation:hover[\s\S]*?background:\s*var\(--ct-hover\)/.test(sidebarStyles), "full-card hover exposes a visible affordance");
  assert(/\.sidebar-team-card-activation:focus-visible[\s\S]*?outline:\s*2px solid var\(--ct-accent\)/.test(sidebarStyles), "full-card activation exposes a visible focus ring");
  assert(card.tagName === "BUTTON" && host.querySelector('[data-testid="team-name"]')?.tagName === "SPAN", "Team title remains a text heading rather than becoming the activation control");
  const originalOpenWorkspace = canvasFocusStore.openWorkspace;
  let boardOpenCount = 0;
  canvasFocusStore.openWorkspace = (target) => {
    boardOpenCount += 1;
    originalOpenWorkspace(target);
  };
  try {
    for (const [index, area] of sampledAreas.entries()) {
      assert(area, `sampled ${["title", "status tally", "lower card"][index]} area renders`);
      canvasFocusStore.closeWorkspace();
      await act(async () => {
        const resolvedTarget = area.getAttribute("data-team-card-hit-target") === card.dataset.testid ? card : area;
        resolvedTarget.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
      });
      assert(boardOpenCount === index + 1, `${["title", "status tally", "lower card"][index]} opens the board exactly once`);
      assert(canvasFocusStore.getWorkspaceTarget()?.planId === currentPlan.id, `${["title", "status tally", "lower card"][index]} opens the bound plan`);
    }
  } finally {
    canvasFocusStore.openWorkspace = originalOpenWorkspace;
  }
  const originalKeyboardOpenWorkspace = canvasFocusStore.openWorkspace;
  let keyboardOpenCount = 0;
  canvasFocusStore.openWorkspace = (target) => {
    keyboardOpenCount += 1;
    originalKeyboardOpenWorkspace(target);
  };
  try {
    for (const key of ["Enter", " "]) {
      canvasFocusStore.closeWorkspace();
      const keydown = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      await act(async () => {
        card.focus();
        card.dispatchEvent(keydown);
        if (!keydown.defaultPrevented) card.click();
      });
      assert(document.activeElement === card, `${key} keeps the card focused`);
      assert(!keydown.defaultPrevented, `${key} keeps native button activation enabled`);
      assert(canvasFocusStore.getWorkspaceTarget()?.planId === currentPlan.id, `${key} opens the bound plan`);
    }
  } finally {
    canvasFocusStore.openWorkspace = originalKeyboardOpenWorkspace;
  }
  assert(keyboardOpenCount === 2, "Enter and Space each open the bound plan exactly once");
  canvasFocusStore.closeWorkspace();
  const memberRow = host.querySelector('[data-testid="tab-item"]') as HTMLElement;
  const memberActivation = memberRow?.querySelector(".sidebar-tab-activation") as HTMLButtonElement;
  assert(memberRow && memberActivation && memberRow.closest(".sidebar-team-member") && !card.contains(memberRow), "member row remains a sibling layered outside the card activation");
  await act(async () => memberActivation.click());
  assert(activatedTabs.join(",") === "orchestrator", "member row keeps its own tab activation handler");
  assert(canvasFocusStore.getWorkspaceTarget() === null, "member row does not open the Team plan through card activation");
  await act(async () => chevron.click());
  assert(chevron.getAttribute("aria-expanded") === "false", "chevron changes only collapse state");
  assert(canvasFocusStore.getWorkspaceTarget() === null, "chevron never opens a plan");
  const ptyWrites: string[] = [];
  const chevronEnter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  await act(async () => {
    chevron.focus();
    const forwarded = forwardTerminalFallback(chevronEnter, document.activeElement, (data) => ptyWrites.push(data));
    assert(!forwarded, "focused Team disclosure handle never forwards Enter to the PTY");
    if (!chevronEnter.defaultPrevented) chevron.click();
  });
  assert(chevron.getAttribute("aria-expanded") === "true", "Enter on the Team disclosure handle keeps native button toggling");
  assert(ptyWrites.length === 0, "Team disclosure handle Enter produces zero PTY writes");
  const terminalFallbackSurface = document.createElement("div");
  terminalFallbackSurface.tabIndex = 0;
  host.append(terminalFallbackSurface);
  const terminalEnter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  terminalFallbackSurface.focus();
  assert(forwardTerminalFallback(terminalEnter, document.activeElement, (data) => ptyWrites.push(data)), "unfocused terminal fallback forwards Enter");
  assert(terminalEnter.defaultPrevented && ptyWrites.join("") === "\r", "terminal fallback preserves its PTY write");
  for (const key of ["Enter", " "]) {
    canvasFocusStore.closeWorkspace();
    if (document.querySelector('[data-testid="team-menu"]')) {
      await act(async () => menu.click());
    }
    await act(async () => {
      const keydown = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      menu.focus();
      menu.dispatchEvent(keydown);
      if (!keydown.defaultPrevented) menu.click();
    });
    assert(document.querySelector('[data-testid="team-menu"]'), `${key} opens the Team menu from its own control`);
    assert(canvasFocusStore.getWorkspaceTarget() === null, `${key} on the Team menu never opens the plan`);
  }
  await act(async () => menu.click());
  await act(async () => {
    header.dispatchEvent(new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: window.innerWidth - 2,
      clientY: window.innerHeight - 2,
    }));
  });
  const contextMenu = document.querySelector('[data-testid="team-menu"]') as HTMLElement | null;
  assert(contextMenu?.getAttribute("role") === "menu", "right-click opens the shared accessible Team menu");
  const contextActions = Array.from(contextMenu.querySelectorAll("button"));
  const openPlan = contextActions.find((button) => button.textContent?.includes("Open Plan")) as HTMLButtonElement | undefined;
  assert(openPlan, "Team context menu exposes Open Plan when a plan is bound");
  assert(
    !contextActions.some((button) => button.textContent?.includes("Rename")),
    "a Team bound to an active plan hides the unrelated Team rename action",
  );
  const menuRectStyle = contextMenu.style;
  assert(Number.parseFloat(menuRectStyle.left) >= 4 && Number.parseFloat(menuRectStyle.top) >= 4, "context menu stays inside viewport margins");
  await act(async () => openPlan.click());
  assert(canvasFocusStore.getWorkspaceTarget()?.planId === currentPlan.id, "context-menu Open Plan targets the bound Team plan");
  assert(!document.querySelector('[data-testid="team-menu"]'), "Open Plan closes the shared menu");

  canvasFocusStore.closeWorkspace();
  await act(async () => menu.click());
  assert(document.querySelector('[data-testid="team-menu"]'), "kebab opens the Team menu");
  assert(document.querySelector('[data-testid="team-menu"]')?.textContent?.includes("Open Plan"), "kebab exposes the same Team actions as right-click");
  assert(canvasFocusStore.getWorkspaceTarget() === null, "kebab never opens a plan");
  await act(async () => root.unmount());

  const unbound = await renderSidebar({
    tabs,
    sortedTabs: [tab("tab-orchestrator", "orchestrator", 0)],
    onRenameTeam: () => {},
  });
  const unboundHeader = unbound.host.querySelector('.sidebar-team-header') as HTMLElement;
  assert(!unbound.host.querySelector('[data-testid="team-card-activation"]'), "unbound Team exposes no deceptive activation surface");
  assert(!unboundHeader.classList.contains("sidebar-team-header-active"), "unbound Team has no pointer or press affordance class");
  await act(async () => unboundHeader.click());
  assert(canvasFocusStore.getWorkspaceTarget() === null, "unbound card click has no plan-open side effect");
  await act(async () => (unbound.host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement).click());
  assert(unbound.host.querySelector('[data-testid="team-collapse-toggle"]')?.getAttribute("aria-expanded") === "false", "unbound chevron remains functional");
  await act(async () => unbound.root.unmount());
});


test("workers_collapse_summary", async () => {
  const tabs = {
    orchestrator: tab({ id: "orchestrator", title: "Orchestrator", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
    workerOne: tab({ id: "workerOne", title: "Worker One", role: "Worker", team_id: TEAM.id }),
    workerTwo: tab({ id: "workerTwo", title: "Worker Two", role: "Worker", team_id: TEAM.id }),
    workerThree: tab({ id: "workerThree", title: "Worker Three", role: "Worker", team_id: TEAM.id }),
  };
  const sortedTabs = Object.keys(tabs).map((id, index) => tab(`tab-${id}`, id, index));
  const { host, root } = await renderSidebar({
    sortedTabs,
    tabs,
    statuses: { workerOne: "Working", workerTwo: "Working", workerThree: "Idle" },
  });
  const toggle = host.querySelector('[data-testid="workers-collapse-toggle"]') as HTMLElement | null;
  assert(toggle, "workers collapse toggle present");
  await act(async () => toggle.click());
  const summary = host.querySelector('[data-testid="workers-collapse-summary"]');
  assert(summary?.textContent?.includes("3 workers · 2 working"), `summary=${summary?.textContent}`);
  await act(async () => root.unmount());
});

test("team worker rows follow tab sort order", async () => {
  const tabs: Record<string, Tab> = {
    orchestrator: tab({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Manager", role_profile: "orchestrator",
      team_id: TEAM.id,
    }),
    workerTwo: tab({ id: "workerTwo", title: "Worker Two", role: "Worker", team_id: TEAM.id }),
    workerOne: tab({ id: "workerOne", title: "Worker One", role: "Worker", team_id: TEAM.id }),
  };
  const sortedTabs = [
    tab("tab-orchestrator", "orchestrator", 0),
    tab("tab-worker-one", "workerOne", 1),
    tab("tab-worker-two", "workerTwo", 2),
  ];
  const { host, root } = await renderSidebar({ tabs, sortedTabs });
  const workerSection = host.querySelector('[data-testid="workers-section"]');
  const rendered = Array.from(workerSection?.querySelectorAll("[data-tab-id]") ?? []).map(
    (element) => element.getAttribute("data-tab-id"),
  );
  assert(rendered.join(",") === "workerOne,workerTwo", "worker DOM order follows tab order");
  await act(async () => root.unmount());
});

test("clicking a team worker activates it", async () => {
  const tabs: Record<string, Tab> = {
    orchestrator: tab({ id: "orchestrator", title: "Orchestrator", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
    worker: tab({ id: "worker", title: "Worker", role: "Worker", team_id: TEAM.id }),
  };
  const sortedTabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-worker", "worker", 1)];
  const workspace: Workspace = { id: "workspace", name: "Workspace", tabs: sortedTabs, active_tab_id: "orchestrator", cwd: null };

  const activations: string[] = [];
  let updateManagerStatus = (_status: TabStatus) => {};

  function Harness() {
    const [activeTabId, setActiveTabId] = useState("orchestrator");
    const [managerStatus, setManagerStatus] = useState<TabStatus>("Working");
    updateManagerStatus = setManagerStatus;
    return (
      <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
        <SidebarWorkspaceTabs
          workspace={workspace}
          activeTabId={activeTabId}
          sortedTabs={sortedTabs.map((tabId) => tabs[tabId]).filter(Boolean)}
          tabs={tabs}
          tabStatuses={{ orchestrator: managerStatus, worker: "Idle" }}
          teams={[TEAM]}
          planIndex={[]}
          plans={[]}
          activeWork={null}
          providers={[]}
          closingTabIds={new Set()}
          appearingTabIds={new Set()}
          isActiveVisual={true}
          workspaceCanvasActive={false}
          touchMode={false}
          compact={false}
          sidebarActionText="text-xs"
          orchestrators={[tabs.orchestrator]}
          onBeforeOpenPlan={() => {}}
          onActivateTab={(tabId) => { activations.push(tabId); setActiveTabId(tabId); }}
          onCloseTab={() => {}}
          onRenameTab={() => {}}
          onNewSession={() => {}}
        />
      </SidebarDnd>
    );
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<Harness />));
  const managerRow = host.querySelector('[data-tab-id="orchestrator"]');
  const managerButton = managerRow?.querySelector('button[aria-label="Open tab Orchestrator"]');
  assert(managerButton, "active manager remains an accessible named button");
  assert(managerButton.getAttribute("aria-current") === "page", "active manager exposes aria-current");
  const managerStatus = managerRow?.querySelector('[role="status"]');
  assert(
    managerStatus?.getAttribute("aria-label") === "Orchestrator tab status Working",
    "working manager status is exposed to assistive technology",
  );
  await act(async () => updateManagerStatus("Idle"));
  assert(
    managerStatus?.getAttribute("aria-label") === "Orchestrator tab status Idle",
    "idle transition updates the assistive status",
  );
  assert(
    managerRow?.querySelector(":scope > .active-tab-pill"),
    "active fill is owned by the row behind the manager button",
  );
  assert(!managerButton.querySelector(".active-tab-pill"), "active fill cannot cover manager content");
  assert(managerButton.textContent?.includes("Orchestrator"), "active manager title remains readable");
  const workerRow = host.querySelector('[data-tab-id="worker"]') as HTMLElement | null;
  assert(workerRow, "worker row rendered");
  assert(workerRow.className.includes("sidebar-tab-dimmed"), "worker starts inactive");
  const workerButton = workerRow.querySelector('button[aria-label="Open tab Worker"]') as HTMLButtonElement | null;
  assert(workerButton, "worker exposes its open action");
  await act(async () => workerButton.click());
  assert(JSON.stringify(activations) === JSON.stringify(["worker"]), "activation receives the tab id exactly once");
  assert(workerButton.getAttribute("aria-current") === "page", "activated worker exposes aria-current");
  assert(managerButton.getAttribute("aria-current") === null, "inactive manager clears aria-current");
  assert(
    !host.querySelector('[data-tab-id="worker"]')?.className.includes("sidebar-tab-dimmed"),
    "worker receives active chrome after click",
  );
  await act(async () => root.unmount());
});

test("waiting team worker keeps prompt pill without status rail", async () => {
  const tabs: Record<string, Tab> = {
    orchestrator: tab({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Manager", role_profile: "orchestrator",
      team_id: TEAM.id,
    }),
    attention: tab({
      id: "attention",
      title: "Attention",
      role: "Worker",
      team_id: TEAM.id,
      agent_type: "claude",
      pending_prompt: {
        question: "Allow team action?",
        options: [{ label: "Yes", value: "yes" }],
      },
    }),
  };
  const sortedTabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-attention", "attention", 1)];
  const { host, root } = await renderSidebar({
    sortedTabs,
    tabs,
    statuses: { attention: "Waiting" },
    activeTabId: "attention",
  });
  const row = host.querySelector('[data-tab-id="attention"]');
  assert(row?.querySelector('span[title="Allow team action?"]'), "prompt pill remains visible");
  assert(!row?.querySelector('[data-testid="tab-status-rail"]'), "waiting worker has no left status rail");
  await act(async () => root.unmount());
});

test("working team worker keeps activity dots without status rail", async () => {
  const tabs: Record<string, Tab> = {
    orchestrator: tab({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Manager", role_profile: "orchestrator",
      team_id: TEAM.id,
    }),
    worker: tab({
      id: "worker",
      title: "Worker",
      role: "Worker",
      team_id: TEAM.id,
      agent_type: "claude",
    }),
  };
  const sortedTabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-worker", "worker", 1)];
  const { host, root } = await renderSidebar({
    sortedTabs,
    tabs,
    statuses: { worker: "Working" },
    activeTabId: "worker",
  });
  const row = host.querySelector('[data-tab-id="worker"]');
  const dots = row?.querySelector('[data-testid="tab-working-activity"] .activity-dots');
  assert(dots?.classList.contains("activity-dots--small"), "working worker uses small activity dots");
  assert(dots?.classList.contains("activity-dots--muted"), "working worker uses muted activity dots");
  assert(!row?.querySelector('[data-testid="tab-status-rail"]'), "working worker has no left status rail");
  await act(async () => root.unmount());
});

test("team worker and manager descriptions render as one subtitle line", async () => {
  const managerDescription = "planning the release cut";
  const workerDescription = "compiling shaders";
  const tabs: Record<string, Tab> = {
    orchestrator: tab({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Manager", role_profile: "orchestrator",
      team_id: TEAM.id,
    }),
    manager: tab({
      id: "manager",
      title: "Planner",
      description: managerDescription,
      role: "Manager",
      role_profile: "planner",
      team_id: TEAM.id,
    }),
    worker: tab({
      id: "worker",
      title: "Worker",
      description: workerDescription,
      role: "Worker",
      team_id: TEAM.id,
    }),
  };
  const sortedTabs = [
    tab("tab-orchestrator", "orchestrator", 0),
    tab("tab-manager", "manager", 1),
    tab("tab-worker", "worker", 2),
  ];
  const { host, root } = await renderSidebar({ tabs, sortedTabs });
  const orchestratorRow = host.querySelector('[data-tab-id="orchestrator"]');
  const managerRow = host.querySelector('[data-tab-id="manager"]');
  const workerRow = host.querySelector('[data-tab-id="worker"]');
  const subtitles = (row: Element | null | undefined) =>
    Array.from(row?.querySelectorAll('[data-testid="tab-description-subtitle"]') ?? []);
  assert(
    subtitles(managerRow).map((node) => node.textContent).join("|") === managerDescription,
    "manager description is exactly one subtitle line",
  );
  assert(
    subtitles(workerRow).map((node) => node.textContent).join("|") === workerDescription,
    "worker description is exactly one subtitle line",
  );
  assert(subtitles(orchestratorRow).length === 0, "a tab without a description gains no subtitle line");
  assert(managerRow?.getAttribute("title")?.includes(managerDescription), "full description stays in the row tooltip");
  assert(!workerRow?.querySelector('[data-testid="tab-subline"]'), "worker still has no role subline");
  assert(managerRow?.querySelectorAll('[data-testid="tab-subline"]').length === 1, "manager has only its role subline");
  await act(async () => root.unmount());
});

test("team worker close control calls onCloseTab", async () => {
  const closed: string[] = [];
  const tabs: Record<string, Tab> = {
    orchestrator: tab({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Manager", role_profile: "orchestrator",
      team_id: TEAM.id,
    }),
    worker: tab({
      id: "worker",
      title: "Worker",
      role: "Worker",
      team_id: TEAM.id,
      agent_type: "claude",
    }),
  };
  const sortedTabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-worker", "worker", 1)];
  const { host, root } = await renderSidebar({ tabs, sortedTabs, onCloseTab: (id) => closed.push(id) });
  const row = host.querySelector('[data-tab-id="worker"]');
  assert(row, "worker row rendered");
  await openTabActionsMenu(row);
  const close = document.querySelector('[data-testid="tab-menu-close"]') as HTMLButtonElement | null;
  assert(close && !close.disabled, "worker close control enabled");
  await act(async () => close.click());
  assert(JSON.stringify(closed) === JSON.stringify(["worker"]), "worker close receives the tab id exactly once");
  await act(async () => root.unmount());
});

test("absent final Orchestrator offers one provider-backed staffing request", async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const provider = (id: string, transport: "pty" | "chat_backend"): ProviderDescriptor => ({
    schema_version: 1,
    id,
    display_name: id,
    transport,
    system_prompt_delivery: "external",
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  } as ProviderDescriptor);
  setAgentDescriptors([
    provider("provider-neutral", "pty"),
    provider("chat-neutral", "chat_backend"),
  ]);
  __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return {};
  }) as never);
  const hubble = tab({
    id: "hubble",
    title: "Hubble",
    role: "Manager",
    role_profile: "orchestrator",
    team_id: TEAM.id,
    pty_id: "pty-hubble",
    backing: { kind: "pty" },
    agent_type: null,
    agent_session_id: "session-hubble",
    boot_stage: "shell_ready",
    agent_booting: false,
  });
  const { host, root } = await renderSidebar({
    tabs: { hubble },
    sortedTabs: [tab("tab-hubble", "hubble", 0)],
  });
  const row = host.querySelector('[data-tab-id="hubble"]');
  assert(row, "Hubble-shaped row rendered");
  await openTabActionsMenu(row);
  const recovery = document.querySelector(
    '[data-testid="tab-menu-replace-orchestrator"]',
  ) as HTMLButtonElement | null;
  assert(recovery, "honestly absent final Orchestrator exposes recovery submenu");
  await act(async () => recovery.click());
  assert(
    !document.querySelector('[data-testid="tab-menu-replace-orchestrator-chat-neutral"]'),
    "chat-backend provider is excluded from PTY staffing",
  );
  const replacement = document.querySelector(
    '[data-testid="tab-menu-replace-orchestrator-provider-neutral"]',
  ) as HTMLButtonElement | null;
  assert(replacement, "PTY provider is available for replacement");
  await act(async () => replacement.click());
  const staffing = calls.filter((call) => call.command === "mux_add_team_member");
  assert(staffing.length === 1, "selection emits exactly one staffing request");
  assert(
    JSON.stringify(staffing[0].args) === JSON.stringify({
      teamId: TEAM.id,
      provider: "provider-neutral",
      role: "orchestrator",
    }),
    "staffing preserves Team and Orchestrator role through the direct-user path",
  );
  await act(async () => root.unmount());
  setAgentDescriptors([]);
  __setInvokeForTest(null);
});

test("Team disclosure stays user-owned across rerenders and remounts", async () => {
  const tabs = {
    orchestrator: tab({ id: "orchestrator", title: "Orchestrator", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
  };
  const sortedTabs = [tab("tab-orchestrator", "orchestrator", 0)];
  const first = await renderSidebar({ tabs, sortedTabs });
  const firstHandle = first.host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement;
  await act(async () => firstHandle.click());
  assert(firstHandle.getAttribute("aria-expanded") === "false", "user collapse takes effect");
  await first.rerender([]);
  assert(firstHandle.getAttribute("aria-expanded") === "false", "data refresh cannot expand a collapsed Team");
  await act(async () => first.root.unmount());

  const refreshed = await renderSidebar({ tabs, sortedTabs });
  const refreshedHandle = refreshed.host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement;
  assert(refreshedHandle.getAttribute("aria-expanded") === "false", "frontend remount restores the collapsed Team state");
  assert(!refreshed.host.querySelector('[data-testid="managers-section"]'), "collapsed Team body stays hidden after remount");
  await act(async () => refreshedHandle.click());
  await act(async () => refreshed.root.unmount());

  const expandedAgain = await renderSidebar({ tabs, sortedTabs });
  assert(
    expandedAgain.host.querySelector('[data-testid="team-collapse-toggle"]')?.getAttribute("aria-expanded") === "true",
    "only a user expansion changes the persisted Team state",
  );
  await act(async () => expandedAgain.root.unmount());
});

test("tab rich menu opens from right click and exposes session actions", async () => {
  const commands: string[] = [];
  const renamed: Array<[string, string]> = [];
  let copied = "";
  __setInvokeForTest((async (command: string) => {
    commands.push(command);
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: async (value: string) => { copied = value; } },
  });
  const tabs: Record<string, Tab> = {
    worker: tab({ id: "worker", title: "Worker", role: "Worker", team_id: TEAM.id, agent_type: "claude", agent_session_id: "session-42" }),
  };
  const { host, root } = await renderSidebar({ tabs, sortedTabs: [tab("tab-worker", "worker", 0)], onRenameTab: (tabId, title) => renamed.push([tabId, title]) });
  const row = host.querySelector('[data-tab-id="worker"]');
  assert(row, "worker row rendered");
  await act(async () => {
    row.dispatchEvent(new MouseEvent("contextmenu", {
      bubbles: true,
      clientX: window.innerWidth - 1,
      clientY: window.innerHeight - 1,
    }));
  });

  const menu = document.querySelector('[role="menu"][aria-label="Tab actions"]') as HTMLDivElement | null;
  assert(menu, "right click opens the same accessible tab actions menu");
  assert(Number.parseFloat(menu.style.left) < window.innerWidth - 4, "right-edge menu is clamped inside the viewport");
  assert(Number.parseFloat(menu.style.top) < window.innerHeight - 4, "bottom-edge menu is clamped inside the viewport");
  assert(Number.parseFloat(menu.style.left) > window.innerWidth / 2, "context menu stays anchored near the right-click edge");
  assert(Number.parseFloat(menu.style.top) > window.innerHeight / 2, "context menu stays anchored near the bottom-click edge");
  const labels = Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent?.trim());
  assert(labels.includes("Rename"), "rename remains available");
  assert(labels.includes("Reconnect"), "reconnect is available");
  assert(labels.includes("Restart session"), "restart is available");
  assert(labels.includes("Close Session"), "close remains available");
  assert(!menu.textContent?.includes("Remove Role"), "legacy role removal is gone");
  assert(!menu.textContent?.includes("Open Terminal"), "legacy terminal toggle is gone");

  const copy = menu.querySelector('[data-testid="tab-menu-copy-session-id"]') as HTMLButtonElement;
  assert(copy.getAttribute("aria-label") === "Copy session ID", "copy icon is named beside the session ID");
  await act(async () => copy.click());
  assert(copied === "session-42", "copy icon writes the session ID");

  const reconnect = Array.from(menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'))
    .find((item) => item.textContent?.trim() === "Reconnect");
  assert(reconnect, "reconnect menu item found");
  await act(async () => reconnect.click());
  assert(!commands.includes("chat_stop"), "reconnect never releases another view's shared chat lease");

  await openTabActionsMenu(row);
  const restart = document.querySelector('[data-testid="tab-menu-restart-session"]') as HTMLButtonElement;
  assert(restart, "three-dot trigger opens the same restart action");
  await act(async () => restart.click());
  assert(commands.includes("restart_agent_tab"), "restart uses the session-preserving backend action");
  const title = host.querySelector('[data-testid="tab-title"]');
  assert(title, "tab title rendered");
  await act(async () => title.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
  const input = host.querySelector('input[aria-label="Rename tab Worker"]') as HTMLInputElement;
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
  await act(async () => { setter?.call(input, "Renamed Worker"); input.dispatchEvent(new Event("input", { bubbles: true })); });
  await act(async () => input.blur());
  assert(JSON.stringify(renamed) === JSON.stringify([["worker", "Renamed Worker"]]), "rename receives the tab id and title exactly once");
  await act(async () => root.unmount());
});

test("rename_space", async () => {
  const renamed: Array<[string, string]> = [];
  const human = tab({ id: "human", title: "Human" });
  const { host, root } = await renderSidebar({
    tabs: { human },
    sortedTabs: ["human"],
    onRenameTab: (tabId, title) => renamed.push([tabId, title]),
  });
  const title = host.querySelector('[data-testid="tab-title"]');
  assert(title, "human tab title rendered");
  await act(async () => title.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
  const input = host.querySelector('input[aria-label="Rename tab Human"]') as HTMLInputElement | null;
  assert(input, "rename editor opened");
  assert(document.activeElement === input, "rename editor receives keyboard focus");

  const space = new KeyboardEvent("keydown", {
    key: " ",
    code: "Space",
    bubbles: true,
    cancelable: true,
  });
  await act(async () => input.dispatchEvent(space));
  assert(!space.defaultPrevented, "Space remains available to the rename input");

  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    setter?.call(input, "Human Tab");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", {
    key: "Enter",
    code: "Enter",
    bubbles: true,
    cancelable: true,
  })));
  assert(JSON.stringify(renamed) === JSON.stringify([["human", "Human Tab"]]), "Enter commits the spaced title");
  await act(async () => root.unmount());
});

test("rename_focus_state", async () => {
  const active = tab({ id: "active", title: "Active" });
  const human = tab({ id: "human", title: "Human" });
  const { host, root } = await renderSidebar({
    tabs: { active, human },
    sortedTabs: ["active", "human"],
    activeTabId: "active",
  });
  const row = host.querySelector('[data-tab-id="human"]') as HTMLElement | null;
  assert(row?.className.includes("sidebar-tab-dimmed"), "inactive tab starts dimmed");
  const title = row?.querySelector('[data-testid="tab-title"]');
  assert(title, "inactive human tab title rendered");
  await act(async () => title.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
  const input = row?.querySelector('input[aria-label="Rename tab Human"]') as HTMLInputElement | null;
  assert(input, "rename editor opened on the inactive tab");
  assert(document.activeElement === input, "rename editor keeps keyboard focus");
  assert(!row.className.includes("sidebar-tab-dimmed"), "editing row keeps normal chrome");
  assert(input.className.includes("ct-input"), "rename editor exposes visible focus styling");

  await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", {
    key: "Escape",
    code: "Escape",
    bubbles: true,
    cancelable: true,
  })));
  assert(!row.querySelector('input[aria-label="Rename tab Human"]'), "Escape closes the rename editor");
  assert(row.className.includes("sidebar-tab-dimmed"), "cancel restores inactive tab chrome");
  await act(async () => root.unmount());
});

test("team manager close control calls onCloseTab", async () => {
  const closed: string[] = [];
  const tabs: Record<string, Tab> = {
    orchestrator: tab({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Manager", role_profile: "orchestrator",
      team_id: TEAM.id,
    }),
    manager: tab({
      id: "manager",
      title: "Planner",
      role: "Manager",
      role_profile: "planner",
      team_id: TEAM.id,
      agent_type: "claude",
    }),
  };
  const sortedTabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-manager", "manager", 1)];
  const { host, root } = await renderSidebar({ tabs, sortedTabs, onCloseTab: (id) => closed.push(id) });
  const row = host.querySelector('[data-tab-id="manager"]');
  assert(row, "manager row rendered");
  await openTabActionsMenu(row);
  const close = document.querySelector('[data-testid="tab-menu-close"]') as HTMLButtonElement | null;
  assert(close && !close.disabled, "manager close control enabled");
  await act(async () => close.click());
  assert(JSON.stringify(closed) === JSON.stringify(["manager"]), "manager close receives the tab id exactly once");
  await act(async () => root.unmount());
});

void run();
