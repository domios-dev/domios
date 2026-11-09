import assert from "node:assert/strict";
import { hydrateProtectedExpanded, initialSidebarExpanded, partitionSidebarWorkspaces, sidebarWorkspaceScrollTop } from "./sidebarSections";

const workspace = (id: string, protectedWorkspace = false) => ({ id, protected: protectedWorkspace }) as never;
const sections = partitionSidebarWorkspaces([workspace("general", true), workspace("codeterm"), workspace("binaura")]);

assert.equal(sections.general?.id, "general", "protected workspace renders under General");
assert.deepEqual(sections.workspaces.map((item) => item.id), ["codeterm", "binaura"], "protected General is absent from Workspaces folders");
assert(initialSidebarExpanded([workspace("general", true), workspace("codeterm")], "codeterm").has("general"), "fresh General starts expanded");
const hydrated = hydrateProtectedExpanded(new Set(), new Set(), [workspace("general", true)]);
assert(hydrated.expanded.has("general"), "hydrated protected General expands after the initially empty store");
const userCollapsed = new Set(hydrated.expanded);
userCollapsed.delete("general");
assert(!hydrateProtectedExpanded(userCollapsed, hydrated.hydratedProtectedIds, [workspace("general", true), workspace("codeterm")]).expanded.has("general"), "workspace refresh preserves a user-collapsed General");
console.log("sidebarSections.test.ts: ok");


const scope = (id: string) => ({ activeWorkspaceId: id, selectedWorkspaceId: id });
for (const retained of [700, 368, 297, 534, 132, 225]) {
  assert.equal(sidebarWorkspaceScrollTop(scope("acp1191"), scope("Quest"), retained), 0,
    "a workspace switch must expose General and the workspace switcher instead of retaining a clipped offset");
}
assert.equal(sidebarWorkspaceScrollTop(scope("Quest"), scope("Quest"), 225), 225, "tab changes and ordinary rerenders keep a deliberate scroll");
assert.equal(sidebarWorkspaceScrollTop(scope("Quest"), { activeWorkspaceId: "general", selectedWorkspaceId: "Quest" }, 225), 0, "opening General must expose its header");
assert.equal(sidebarWorkspaceScrollTop(scope("Quest"), { activeWorkspaceId: "Quest", selectedWorkspaceId: "gem1191" }, 225), 0, "changing only the workspace detail must expose the switcher");
