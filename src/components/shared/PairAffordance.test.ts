// Run: npx tsx src/components/shared/PairAffordance.test.ts

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, "PairAffordance.tsx"), "utf8");
const sidebar = readFileSync(join(here, "..", "Sidebar", "SidebarPlanPeerPair.tsx"), "utf8");
const sidebarWorkspace = readFileSync(join(here, "..", "Sidebar", "SidebarWorkspaceTabs.tsx"), "utf8");
const header = readFileSync(
  join(here, "..", "Canvas", "planView", "PlanContextHeader.tsx"),
  "utf8",
);
const hierarchy = readFileSync(join(here, "..", "Sidebar", "HierarchyTree.tsx"), "utf8");
const css = readFileSync(join(here, "..", "..", "styles", "globals.css"), "utf8");

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(source.includes('data-pair-role="orchestrator"'), "PairAffordance labels orchestrator slot");
assert(source.includes('data-pair-role="planner"'), "PairAffordance labels planner slot");
assert(source.includes("pair-affordance--${layout}"), "layout-specific class");
assert(source.includes('data-pair-affordance-layout={layout}'), "layout data attribute");

assert(sidebar.includes("resolvePlanPeerPair"), "Sidebar derives pair from plan refs");
assert(sidebarWorkspace.includes("peerExclude"), "Sidebar excludes planner from worker subtree");
// it uses the Managers/Workers section taxonomy instead (round-21).
assert(
  sidebarWorkspace.includes("OrchestrationGroupSection") &&
    sidebarWorkspace.includes('section="manager"') &&
    sidebarWorkspace.includes('section="worker"'),
  "Sidebar renders the orchestration group as Managers/Workers sections",
);
assert(
  !sidebar.includes("<PairAffordance") && !sidebarWorkspace.includes("<PairAffordance"),
  "Sidebar stopped rendering the same-level PairAffordance pair",
);

assert(header.includes("<PairAffordance"), "PlanContextHeader uses PairAffordance");
assert(header.includes('data-testid="plan-peer-pair"'), "plan header orchestration marker");

assert(hierarchy.includes("excludePaneIds"), "Hierarchy subtree can exclude attached Planner");

assert(!css.includes(".pair-affordance-rail"), "dead sidebar rail styles removed");
assert(!source.includes('layout === "sidebar"'), "PairAffordance sidebar layout removed");

console.log("✓ PairAffordance: chips/card only; sidebar uses Managers/Workers sections");
