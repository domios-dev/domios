import assert from "node:assert/strict";
import {
  hydrateProtectedExpanded,
  initialSidebarExpanded,
  minimalScrollDeltaForItem,
  minimalScrollTopForItem,
  partitionSidebarWorkspaces,
} from "./sidebarSections";

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

const viewport = { top: 100, bottom: 500 };
assert.equal(minimalScrollDeltaForItem(viewport, { top: 200, bottom: 240 }), 0, "visible item requires no scroll");
assert.equal(minimalScrollTopForItem(280, viewport, { top: 200, bottom: 240 }), 280, "visible item preserves scrollTop");
assert.equal(minimalScrollTopForItem(280, viewport, { top: 40, bottom: 80 }), 220, "item above viewport scrolls minimally up instead of jumping to top");
assert.equal(minimalScrollTopForItem(280, viewport, { top: 85, bottom: 125 }), 265, "partially clipped top item scrolls minimally up");
assert.equal(minimalScrollTopForItem(120, viewport, { top: 530, bottom: 570 }), 190, "item below viewport scrolls minimally down");
assert.equal(minimalScrollTopForItem(30, viewport, { top: 40, bottom: 80 }), 0, "minimal scroll clamps at 0");
assert.equal(minimalScrollTopForItem(280, { top: 0, bottom: 0 }, { top: 20, bottom: 60 }), 280, "unmeasured viewport never changes scrollTop");
assert.equal(minimalScrollTopForItem(280, viewport, { top: 0, bottom: 0 }), 280, "unmeasured item never changes scrollTop");
console.log("sidebarSections.test.ts: ok");
