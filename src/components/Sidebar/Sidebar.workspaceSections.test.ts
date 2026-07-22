import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { initialSidebarExpanded } from "./sidebarSections";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
Object.defineProperty(globalThis, "sessionStorage", { value: window.sessionStorage, configurable: true });

async function run() {
  const {
    SIDEBAR_WORKSPACES_SECTION_EXPANDED_KEY,
    loadWorkspacesSectionExpanded,
    persistWorkspacesSectionExpanded,
    toggleWorkspacesSectionExpanded,
    workspaceSectionBodyVisible,
  } = await import("./Sidebar");

  const ordinaryWorkspaceId = "ordinary";
  const ordinaryInitial = initialSidebarExpanded([
    { id: "general", protected: true },
    { id: ordinaryWorkspaceId, protected: false },
  ] as never, ordinaryWorkspaceId);
  assert(ordinaryInitial.has("general"), "existing General initialization remains intact");
  assert(ordinaryInitial.has(ordinaryWorkspaceId), "existing active workspace initialization remains intact");
  assert(loadWorkspacesSectionExpanded(), "full sidebar initially renders WORKSPACES body");

  const collapsed = toggleWorkspacesSectionExpanded(true);
  persistWorkspacesSectionExpanded(collapsed);
  assert(!collapsed, "WORKSPACES click collapses and aria-expanded becomes false");
  assert(!workspaceSectionBodyVisible(false, collapsed), "collapsed full sidebar hides WORKSPACES body");

  const reloaded = loadWorkspacesSectionExpanded();
  assert(!reloaded, "WORKSPACES collapse persists across remount/reload");
  const ordinaryReloaded = initialSidebarExpanded([
    { id: "general", protected: true },
    { id: ordinaryWorkspaceId, protected: false },
    { id: "new-workspace", protected: false },
  ] as never, "new-workspace");
  assert(ordinaryReloaded.has("new-workspace"), "section preference does not overwrite a newly active workspace expansion");
  assert(!ordinaryReloaded.has("__workspaces__"), "section preference never stores ordinary workspace identifiers");

  const restored = toggleWorkspacesSectionExpanded(reloaded);
  persistWorkspacesSectionExpanded(restored);
  assert(restored, "WORKSPACES click expands and aria-expanded becomes true");
  assert(workspaceSectionBodyVisible(false, restored), "expanded full sidebar restores WORKSPACES body");

  const compactCollapsed = toggleWorkspacesSectionExpanded(restored);
  persistWorkspacesSectionExpanded(compactCollapsed);
  assert(workspaceSectionBodyVisible(true, compactCollapsed), "compact sidebar always renders WORKSPACES body when full mode is persisted collapsed");
  assert(!workspaceSectionBodyVisible(false, compactCollapsed), "returning to full mode restores the persisted collapsed state");
  assert.equal(window.localStorage.getItem(SIDEBAR_WORKSPACES_SECTION_EXPANDED_KEY), "0", "section preference writes only one boolean");

  console.log("Sidebar.workspaceSections.test.ts: ok");
}

void run();
