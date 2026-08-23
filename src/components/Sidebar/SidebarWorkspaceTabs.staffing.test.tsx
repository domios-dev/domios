// Run: npx tsx src/components/Sidebar/SidebarWorkspaceTabs.staffing.test.tsx
// Staffing: which managers are eligible, what the picker offers, and how a
// failed or double-clicked provider action behaves.

import type { ProviderDescriptor } from "../../lib/types";
import type { TeamMemberRole } from "../../lib/teamActions";
import { canvasFocusStore } from "../Canvas/canvasFocusStore";
import { tab, TEAM } from "./SidebarWorkspaceTabs.fixtures.testdata";
import {
  act,
  assert,
  createSuite,
  renderSidebar,
  setupDom,
  sidebarStyles,
} from "./SidebarWorkspaceTabs.testkit";

const { test, run } = createSuite("SidebarWorkspaceTabs staffing");

setupDom();

test("team menu enables staffing only for a Team with a manager", async () => {
  const closed: string[] = [];
  const tabs = {
    manager: tab({ id: "manager", title: "Manager", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
  };
  canvasFocusStore.closeWorkspace();
  const { host, root } = await renderSidebar({
    tabs,
    sortedTabs: [tab("tab-manager", "manager", 0)],
    onRenameTeam: () => {},
    onAddTeamMember: () => {},
    onCloseTeam: (teamId) => closed.push(teamId),
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement | null;
  assert(trigger, "team menu trigger exists");
  assert(trigger.classList.contains("sidebar-team-kebab"), "team menu uses the same-slot kebab control");
  assert(trigger.tabIndex === 0, "team menu trigger remains keyboard reachable for the focus swap");
  assert(/\.sidebar-team-unit\s*\{[^}]*margin:\s*4px 0;[^}]*background:\s*color-mix\(in srgb, var\(--ct-fg\) 3%, transparent\);/.test(sidebarStyles), "Team cards use lighter tint and compact vertical cadence");
  assert(/\.sidebar-team-header\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\) auto;[^}]*gap:\s*6px;[^}]*margin:\s*0;[^}]*padding:\s*6px 7px 4px 10px;/.test(sidebarStyles), "Team header reclaims the former chevron column");
  assert(/\.sidebar-team-disclosure-bar\s*\{[^}]*position:\s*relative;[^}]*display:\s*block;[^}]*width:\s*100%;[^}]*height:\s*14px;/.test(sidebarStyles), "Team disclosure is a compact full-width bottom handle");
  assert(/\.sidebar-team-disclosure-glyph\s*\{[^}]*left:\s*50%;[^}]*transform:\s*translate\(-50%, -50%\);/.test(sidebarStyles), "Team disclosure glyph stays geometrically centered");
  assert(/\.sidebar-team-disclosure-glyph::before,\s*\.sidebar-team-disclosure-glyph::after\s*\{[^}]*width:\s*17px;[^}]*height:\s*1px;/.test(sidebarStyles), "Team disclosure uses a thin, restrained line");
  assert(sidebarStyles.includes("right: calc(50% - 0.5px);") && sidebarStyles.includes("left: calc(50% - 0.5px);"), "Team disclosure overlaps its two segments at the joint");
  assert(sidebarStyles.includes("transform: rotate(-8deg);"), "Team disclosure uses a gentle arrow transition");
  assert(/\.sidebar-team-disclosure-label,\s*\.sidebar-team-disclosure-count\s*\{[^}]*left:\s*calc\(50% \+ 24px\);/.test(sidebarStyles), "Team disclosure text cannot shift the centered glyph");
  assert(/\.ct-plan-status-breakdown\s*\{[^}]*flex-wrap:\s*nowrap;[^}]*white-space:\s*nowrap;/.test(sidebarStyles), "status breakdown stays on one line");
  const header = host.querySelector('.sidebar-team-header') as HTMLDivElement | null;
  const chevron = host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement | null;
  assert(!header?.hasAttribute("role") && header?.tabIndex === -1, "Team header is a non-interactive control container");
  assert(chevron?.getAttribute("aria-expanded") === "true", "Team bottom handle exposes the expanded state");
  assert(chevron?.querySelector(".sidebar-team-disclosure-glyph"), "Team bottom handle renders the animated line glyph");
  assert(chevron?.querySelector(".sidebar-team-disclosure-label")?.textContent?.trim() === "Hide", "expanded Team exposes the hover Hide label");
  const managersSection = host.querySelector('[data-testid="managers-section"]');
  assert(Boolean(chevron && managersSection && (managersSection.compareDocumentPosition(chevron) & Node.DOCUMENT_POSITION_FOLLOWING)), "Team disclosure remains below the manager section");
  await act(async () => chevron?.click());
  assert(chevron?.getAttribute("aria-expanded") === "false", "bottom handle click collapses Team");
  assert(chevron?.querySelector(".sidebar-team-disclosure-label")?.textContent?.trim() === "Show", "collapsed Team exposes the hover Show label");
  assert(chevron?.querySelector(".sidebar-team-disclosure-count")?.textContent?.trim() === "1 tab", "collapsed Team shows its tab count beside the centered handle");
  await act(async () => chevron?.click());
  assert(chevron?.getAttribute("aria-expanded") === "true", "bottom handle click expands Team");
  assert(/\.sidebar-team-progress-slot\s*\{[^}]*width:\s*24px;[^}]*height:\s*20px;/.test(sidebarStyles), "done/total and kebab share one fixed slot");
  assert(/\.ct-plan-status-breakdown-separator\s*\{[^}]*margin:\s*0 2px;/.test(sidebarStyles), "status separators have explicit spacing");
  assert(/\.sidebar-team-menu-root \.sidebar-team-kebab\s*\{[^}]*opacity:\s*0;/.test(sidebarStyles), "kebab hides by opacity without removing its slot");
  assert(/\.sidebar-team-menu-root \.sidebar-team-kebab\s*\{[^}]*opacity:\s*0;[^}]*pointer-events:\s*none;/.test(sidebarStyles), "hidden kebab ignores accidental pointer clicks");
  assert(sidebarStyles.includes(".sidebar-team-header:hover .sidebar-team-menu-root .sidebar-team-kebab"), "CSS hover swaps the kebab into the fixed slot");
  assert(sidebarStyles.includes('.sidebar-team-header:not([data-menu-dismissed="true"]):focus-within .sidebar-team-menu-root .sidebar-team-kebab'), "keyboard focus swaps the kebab into the fixed slot unless an outside pointer dismisses it");
  assert(sidebarStyles.includes("[data-touch-mode=\"true\"] .sidebar-team-menu-root .sidebar-team-kebab"), "touch mode persistently exposes the same-slot kebab");
  assert(!sidebarStyles.includes("data-menu-revealed"), "hover visibility has no React state attribute");
  const outsideFocus = document.createElement("button");
  host.appendChild(outsideFocus);
  await act(async () => {
    outsideFocus.focus();
    trigger.focus();
  });
  assert(!header?.hasAttribute("data-menu-dismissed"), "keyboard focus inside the header keeps the menu reveal eligible");
  await act(async () => document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })));
  assert(header?.getAttribute("data-menu-dismissed") === "true", "outside pointerdown dismisses the menu despite retained header focus");
  await act(async () => {
    outsideFocus.focus();
    trigger.focus();
  });
  assert(!header?.hasAttribute("data-menu-dismissed"), "a later keyboard focus restores the focus-within reveal");
  await act(async () => trigger.click());
  const menu = document.querySelector('[data-testid="team-menu"]');
  assert(menu?.textContent?.includes("Rename"), "supported rename remains available");
  assert(menu?.textContent?.includes("Add worker"), "worker staffing is enabled with a handler and manager");
  assert(menu?.textContent?.includes("Add planner"), "planner staffing is enabled with a handler and manager");
  assert(menu?.textContent?.includes("Add orchestrator"), "orchestrator staffing is enabled with a handler and manager");
  assert(!menu?.textContent?.includes("Create effort"), "humans cannot create efforts from Team UI");
  assert(!menu?.textContent?.includes("Open effort"), "no effort action is shown without a plan");
  const close = Array.from(menu?.querySelectorAll("button") ?? []).find((button) => button.textContent?.includes("Close Team")) as HTMLButtonElement | undefined;
  assert(close, "Close Team action is available");
  await act(async () => close.click());
  assert(closed.join(",") === TEAM.id, "Close Team reports the exact Team id");
  assert(host.querySelector('[data-testid="team-icon"]'), "Team identity remains visible without a plan");
  assert(!host.querySelector('[data-testid="team-plan-button"]'), "no plan button is rendered without a plan");
  assert(!host.querySelector('[data-testid="team-status-tally"]'), "no plan tally is rendered without a plan");
  canvasFocusStore.closeWorkspace();
  await act(async () => root.unmount());
});

test("managerless Team hides staffing and makes no request", async () => {
  const requests: unknown[] = [];
  const tabs = { worker: tab({ id: "worker", title: "Worker", role: "Worker", team_id: TEAM.id }) };
  const { host, root } = await renderSidebar({
    tabs,
    sortedTabs: [tab("tab-worker", "worker", 0)],
    onAddTeamMember: (...args) => requests.push(args),
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement | null;
  assert(trigger, "team menu trigger exists");
  await act(async () => trigger.click());
  const menu = document.querySelector('[data-testid="team-menu"]');
  for (const role of ["worker", "planner", "orchestrator"])
    assert(!menu?.textContent?.includes(`Add ${role}`), `managerless Team hides ${role} staffing`);
  assert(!menu?.textContent?.includes("Rename"), "managerless Team hides rename");
  assert(requests.length === 0, "managerless Team does not issue a staffing request");
  await act(async () => root.unmount());
});

test("staffing selects the first eligible manager in visible Orchestrator-first order", async () => {
  const requests: Array<[string, TeamMemberRole, string, string]> = [];
  const tabs = {
    unrelated: tab({ id: "unrelated", title: "Unrelated Tab", role: "Worker" }),
    "a-O2": tab({ id: "a-O2", title: "Orchestrator Two", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
    "p-P1": tab({ id: "p-P1", title: "Planner One", role: "Manager", role_profile: "planner", team_id: TEAM.id }),
    "z-O1": tab({ id: "z-O1", title: "Orchestrator One", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
  };
  const providers = [{
    id: "registry-provider",
    display_name: "Registry provider",
    transport: "pty",
  }, {
    id: "not-spawn-capable",
    display_name: "Unsupported provider",
    transport: "unsupported",
  }] as ProviderDescriptor[];
  const { host, root } = await renderSidebar({
    tabs,
    sortedTabs: [tab("tab-unrelated", "unrelated", 0), tab("tab-O1", "z-O1", 1), tab("tab-O2", "a-O2", 2), tab("tab-P1", "p-P1", 3)],
    activeTabId: "tab-unrelated",
    providers,
    onAddTeamMember: (teamId, role, providerId, managerTabId) => {
      requests.push([teamId, role, providerId, managerTabId]);
    },
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement | null;
  assert(trigger, "team menu trigger exists");
  await act(async () => trigger.click());
  const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement | undefined;
  assert(worker, "worker staffing action exists");
  await act(async () => worker.click());
  const provider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Registry provider")) as HTMLButtonElement | undefined;
  assert(provider, "spawn-capable registry provider is offered");
  assert(
    !document.querySelector('[data-testid="team-menu"]')?.textContent?.includes("Unsupported provider"),
    "non-spawn-capable registry providers are absent",
  );
  await act(async () => provider.click());
  assert(
    JSON.stringify(requests) === JSON.stringify([[TEAM.id, "worker", "registry-provider", "z-O1"]]),
    "the first visible Orchestrator is passed as caller identity while an unrelated tab is active",
  );
  await act(async () => root.unmount());
});

test("staffing selects a planner when no Orchestrator is eligible", async () => {
  const requests: Array<[string, TeamMemberRole, string, string]> = [];
  const tabs = {
    watcher: tab({ id: "A0", title: "Watcher", role: "Manager", role_profile: "watcher", team_id: TEAM.id }),
    planner: tab({ id: "P1", title: "Planner", role: "Manager", role_profile: "planner", team_id: TEAM.id, can_spawn: true }),
  };
  const providers = [{ id: "registry-provider", display_name: "Registry provider", transport: "pty" }] as ProviderDescriptor[];
  const { host, root } = await renderSidebar({
    tabs,
    sortedTabs: [tab("tab-watcher", "watcher", 0), tab("tab-planner", "planner", 1)],
    providers,
    onAddTeamMember: (teamId, role, providerId, managerTabId) => requests.push([teamId, role, providerId, managerTabId]),
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement;
  await act(async () => trigger.click());
  const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
  await act(async () => worker.click());
  const provider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Registry provider")) as HTMLButtonElement;
  await act(async () => provider.click());
  assert(
    JSON.stringify(requests) === JSON.stringify([[TEAM.id, "worker", "registry-provider", "P1"]]),
    "a spawn-capable planner is selected while a leaf manager is skipped",
  );
  await act(async () => root.unmount());
});

test("staffing picker uses descriptor icons and requires a descriptor model choice", async () => {
  const requests: Array<[string, TeamMemberRole, string, string, string?]> = [];
  const tabs = {
    manager: tab({ id: "manager", title: "Manager", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
  };
  const providers = [{
    id: "provider-a",
    display_name: "Provider A",
    transport: "pty",
    icon_svg: '<svg id="provider-a-icon"/>',
    icon_color: "#d97757",
    available_models: [{ id: "model-one", display_name: "Model one" }, { id: "model-two", display_name: "Model two" }],
    default_model: "model-one",
    behavior_config: { model: "own_prompt_set" },
  }, {
    id: "provider-b",
    display_name: "Provider B",
    transport: "chat_backend",
    icon_hint: "B",
    behavior_config: { model: "none" },
  }, {
    id: "not-spawn-capable",
    display_name: "Unsupported provider",
    transport: "unsupported",
  }] as ProviderDescriptor[];
  const { host, root } = await renderSidebar({
    tabs,
    sortedTabs: [tab("tab-manager", "manager", 0)],
    providers,
    onAddTeamMember: (teamId, role, providerId, managerTabId, model) => requests.push([teamId, role, providerId, managerTabId, model]),
  });
  await act(async () => (host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click());
  const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
  await act(async () => worker.click());
  const providerA = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider A")) as HTMLButtonElement;
  assert(providerA.querySelector("svg#provider-a-icon"), "provider SVG comes from its descriptor");
  const providerB = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider B")) as HTMLButtonElement;
  assert(providerB.querySelector(".agent-icon")?.textContent === "B", "provider hint is the descriptor icon fallback");
  assert(!document.querySelector('[data-testid="team-menu"]')?.textContent?.includes("Unsupported provider"), "non-spawn-capable providers are absent");
  await act(async () => providerA.click());
  const defaultModel = document.querySelector('[data-testid="team-picker-model-model-one"]') as HTMLButtonElement;
  assert(defaultModel.getAttribute("aria-checked") === "true", "descriptor default model is preselected");
  const modelTwo = document.querySelector('[data-testid="team-picker-model-model-two"]') as HTMLButtonElement;
  await act(async () => modelTwo.click());
  assert(JSON.stringify(requests) === JSON.stringify([[TEAM.id, "worker", "provider-a", "manager", "model-two"]]), "model selection preserves manager caller identity");
  await act(async () => (host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click());
  const workerAgain = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
  await act(async () => workerAgain.click());
  const providerAgain = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider A")) as HTMLButtonElement;
  await act(async () => providerAgain.click());
  const back = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Back")) as HTMLButtonElement;
  await act(async () => back.click());
  const restoredProvider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider A")) as HTMLButtonElement;
  assert(document.activeElement === restoredProvider, "Back restores focus to the selected provider");
  await act(async () => restoredProvider.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
  const restoredProviderB = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider B")) as HTMLButtonElement;
  assert(document.activeElement === restoredProviderB, "arrow navigation reaches every provider row");
  await act(async () => restoredProviderB.click());
  assert(JSON.stringify(requests) === JSON.stringify([
    [TEAM.id, "worker", "provider-a", "manager", "model-two"],
    [TEAM.id, "worker", "provider-b", "manager", undefined],
  ]), "model:none provider skips model selection and omits model from the request");
  await act(async () => (host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click());
  const menu = document.querySelector('[data-testid="team-menu"]') as HTMLDivElement;
  await act(async () => menu.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  assert(!document.querySelector('[data-testid="team-menu"]'), "Escape closes the picker without another request");
  await act(async () => root.unmount());
});

test("staffing failure stays visible without local mutation or refetch", async () => {
  const tabs = {
    manager: tab({ id: "manager", title: "Manager", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
  };
  const tabsBefore = JSON.stringify(tabs);
  const providers = [{ id: "registry-provider", display_name: "Registry provider", transport: "pty" }] as ProviderDescriptor[];
  const originalFetch = globalThis.fetch;
  let fetches = 0;
  globalThis.fetch = async () => {
    fetches += 1;
    throw new Error("unexpected refetch");
  };
  try {
    const { host, root } = await renderSidebar({
      tabs,
      sortedTabs: [tab("tab-manager", "manager", 0)],
      providers,
      onAddTeamMember: async () => {
        throw new Error("Team manager permission denied (403)");
      },
    });
    const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement;
    await act(async () => trigger.click());
    const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
      .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
    await act(async () => worker.click());
    const provider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
      .find((button) => button.textContent?.includes("Registry provider")) as HTMLButtonElement;
    await act(async () => provider.click());
    assert(
      document.querySelector('[data-testid="team-menu-error"]')?.textContent?.includes("403"),
      "403 staffing rejection is shown inline",
    );
    assert(JSON.stringify(tabs) === tabsBefore, "staffing rejection does not mutate local Tab state");
    assert(fetches === 0, "staffing rejection does not refetch or poll");
    await act(async () => root.unmount());
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("staffing provider action ignores an immediate double click", async () => {
  let invocations = 0;
  let finish: (() => void) | undefined;
  const tabs = {
    manager: tab({ id: "manager", title: "Manager", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id }),
  };
  const providers = [{ id: "registry-provider", display_name: "Registry provider", transport: "pty" }] as ProviderDescriptor[];
  const { host, root } = await renderSidebar({
    tabs,
    sortedTabs: [tab("tab-manager", "manager", 0)],
    providers,
    onAddTeamMember: () => {
      invocations += 1;
      return new Promise<void>((resolve) => {
        finish = resolve;
      });
    },
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement;
  await act(async () => trigger.click());
  const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
  await act(async () => worker.click());
  const provider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Registry provider")) as HTMLButtonElement;
  await act(async () => {
    provider.click();
    provider.click();
  });
  assert(invocations === 1, "two immediate clicks issue exactly one staffing request");
  assert(provider.disabled, "provider action is disabled while staffing is pending");
  finish?.();
  await act(async () => {});
  assert(!document.querySelector('[data-testid="team-menu"]'), "menu closes after successful staffing");
  await act(async () => root.unmount());
});

void run();
