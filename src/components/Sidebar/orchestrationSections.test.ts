// Run: npx tsx src/components/Sidebar/orchestrationSections.test.ts

import type { Tab } from "../../lib/types";
import {
  countBySection,
  partitionBySection,
  roleBadgeLabel,
  roleShieldLabel,
  roleBadgeTestId,
  SECTION_LABEL,
  sortTeamManagers,
  sectionForRole,
  summarizeGroupClose,
  summarizeWorkers,
} from "./orchestrationSections";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function tab(overrides: Partial<Tab> = {}): Tab {
  return { id: "p", title: "p", role: null, role_profile: null, ...overrides } as Tab;
}

const debugCalls: string[] = [];
const origDebug = console.debug;
console.debug = (...args: unknown[]) => {
  debugCalls.push(args.join(" "));
};
try {
  assert(sectionForRole("Manager", "planner") === "manager", "planner Manager -> manager");
  assert(sectionForRole("Manager", "watcher") === "manager", "watcher Manager -> manager");
  assert(sectionForRole("Worker", "planner") === "worker", "Worker profile does not make a manager");
  assert(sectionForRole("Worker", "worker") === "worker", "worker -> worker");
  assert(sectionForRole(null, null) === "worker", "null -> worker");
  assert(sectionForRole(undefined, undefined) === "worker", "undefined -> worker");
  assert(sectionForRole("Worker", "") === "worker", "empty -> worker");
  const before = debugCalls.length;
  assert(sectionForRole("Worker", "reviewer") === "worker", "unknown worker profile -> worker fallback");
  assert(debugCalls.length === before + 1, "unknown role logs exactly one debug breadcrumb");
  assert(debugCalls[before].includes("reviewer"), "debug breadcrumb names the unknown role");
  // Known buckets never spam the log.
  const q = debugCalls.length;
  sectionForRole("Manager", "planner");
  sectionForRole("Worker", "worker");
  sectionForRole(null, null);
  assert(debugCalls.length === q, "known/empty roles never log");
} finally {
  console.debug = origDebug;
}

assert(roleBadgeLabel(tab({ role: "Orchestrator" })) === "ORCHESTRATOR", "orchestrator badge");
assert(
  roleBadgeLabel(tab({ role: "Manager", role_profile: "planner" })) === "PLANNER",
  "planner badge",
);
assert(
  roleBadgeLabel(tab({ role: "Manager", role_profile: "watcher" })) === "WATCHER",
  "watcher badge (future)",
);
assert(roleBadgeLabel(tab({ role: "Worker" })) === null, "plain worker has no leadership badge label");
assert(roleShieldLabel(tab({ role: "Worker" })) === null, "workers have no role shield");
assert(roleShieldLabel(tab({ role: "Orchestrator" })) === "ORCHESTRATOR", "orchestrator shield");
assert(roleShieldLabel(tab({ role: "Manager", role_profile: "planner" })) === "PLANNER", "planner shield");
assert(roleShieldLabel(null) === null, "null Tab → no shield");
assert(roleBadgeTestId("ORCHESTRATOR") === "role-badge-orchestrator", "orchestrator testid");
assert(roleBadgeTestId("PLANNER") === "role-badge-planner", "planner testid");

const nodes = [
  { tab: tab({ id: "w1", role: "Worker", role_profile: null }) },
  { tab: tab({ id: "planner", role: "Manager", role_profile: "planner" }) },
  { tab: tab({ id: "w2", role: "Worker", role_profile: "worker" }) },
];
const part = partitionBySection(nodes);
assert(part.managers.map((n) => n.tab.id).join(",") === "planner", "managers = planner only");
assert(part.workers.map((n) => n.tab.id).join(",") === "w1,w2", "workers keep spawn order");
const sortedManagers = sortTeamManagers([
  { tab: tab({ id: "planner-1", role: "Manager", role_profile: "planner" }) },
  { tab: tab({ id: "orch-1", role: "Orchestrator" }) },
  { tab: tab({ id: "planner-2", role: "Manager", role_profile: "planner" }) },
  { tab: tab({ id: "orch-2", role: "Orchestrator" }) },
]);
assert(
  sortedManagers.map((node) => node.tab.id).join(",") === "orch-1,orch-2,planner-1,planner-2",
  "team managers sort orchestrators first and preserve manual order within role",
);

assert(SECTION_LABEL.manager === "Managers" && SECTION_LABEL.worker === "Workers", "section labels");
const counts = countBySection([
  { role: "Manager", roleProfile: "planner" },
  { role: "Worker", roleProfile: "worker" },
  { role: "Worker", roleProfile: null },
  { role: "Worker", roleProfile: "worker" },
]);
assert(counts.managers === 1 && counts.workers === 3, "counts split by section");
assert(summarizeGroupClose(counts) === "1 manager, 3 workers", "singular manager, plural workers");
assert(summarizeGroupClose({ managers: 0, workers: 2 }) === "2 workers", "omits zero managers");
assert(summarizeGroupClose({ managers: 2, workers: 0 }) === "2 managers", "omits zero workers");
assert(summarizeGroupClose({ managers: 1, workers: 1 }) === "1 manager, 1 worker", "both singular");
assert(summarizeWorkers(3, 2) === "3 workers · 2 working", "worker collapse summary");

console.log("✓ orchestrationSections: role→section matrix, badges, partition, group-close summary");
