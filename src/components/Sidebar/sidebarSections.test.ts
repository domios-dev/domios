import assert from "node:assert/strict";
import { hydrateProtectedExpanded, initialSidebarExpanded, partitionSidebarWorkspaces } from "./sidebarSections";

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
