// Run: npx tsx src/components/Sidebar/orchestrationSections.test.ts

import type { Pane } from "../../lib/types";
import {
  countBySection,
  partitionBySection,
  roleBadgeLabel,
  roleBadgeTestId,
  SECTION_LABEL,
  sectionForRoleProfile,
  summarizeGroupClose,
} from "./orchestrationSections";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function pane(overrides: Partial<Pane> = {}): Pane {
  return { id: "p", title: "p", role: null, role_profile: null, ...overrides } as Pane;
}

const debugCalls: string[] = [];
const origDebug = console.debug;
console.debug = (...args: unknown[]) => {
  debugCalls.push(args.join(" "));
};
try {
  assert(sectionForRoleProfile("planner") === "manager", "planner → manager");
  assert(sectionForRoleProfile("Planner") === "manager", "case-insensitive planner → manager");
  assert(sectionForRoleProfile(" watcher ") === "manager", "watcher (future) → manager");
  assert(sectionForRoleProfile("worker") === "worker", "worker → worker");
  assert(sectionForRoleProfile(null) === "worker", "null → worker");
  assert(sectionForRoleProfile(undefined) === "worker", "undefined → worker");
  assert(sectionForRoleProfile("") === "worker", "empty → worker");
  const before = debugCalls.length;
  assert(sectionForRoleProfile("reviewer") === "worker", "unknown role → worker fallback");
  assert(debugCalls.length === before + 1, "unknown role logs exactly one debug breadcrumb");
  assert(debugCalls[before].includes("reviewer"), "debug breadcrumb names the unknown role");
  // Known buckets never spam the log.
  const q = debugCalls.length;
  sectionForRoleProfile("planner");
  sectionForRoleProfile("worker");
  sectionForRoleProfile(null);
  assert(debugCalls.length === q, "known/empty roles never log");
} finally {
  console.debug = origDebug;
}

assert(roleBadgeLabel(pane({ role: "Orchestrator" })) === "ORCHESTRATOR", "orchestrator badge");
assert(
  roleBadgeLabel(pane({ role: "Worker", role_profile: "planner" })) === "PLANNER",
  "planner badge",
);
assert(
  roleBadgeLabel(pane({ role: "Worker", role_profile: "watcher" })) === "WATCHER",
  "watcher badge (future)",
);
assert(roleBadgeLabel(pane({ role: "Worker" })) === null, "plain worker has no role badge");
assert(roleBadgeLabel(pane({ role: "Worker", role_profile: "reviewer" })) === null, "unknown worker → no badge");
assert(roleBadgeLabel(null) === null, "null pane → no badge");
assert(roleBadgeTestId("ORCHESTRATOR") === "role-badge-orchestrator", "orchestrator testid");
assert(roleBadgeTestId("PLANNER") === "role-badge-planner", "planner testid");

const nodes = [
  { pane: pane({ id: "w1", role_profile: null }) },
  { pane: pane({ id: "planner", role_profile: "planner" }) },
  { pane: pane({ id: "w2", role_profile: "worker" }) },
];
const part = partitionBySection(nodes);
assert(part.managers.map((n) => n.pane.id).join(",") === "planner", "managers = planner only");
assert(part.workers.map((n) => n.pane.id).join(",") === "w1,w2", "workers keep spawn order");

assert(SECTION_LABEL.manager === "Managers" && SECTION_LABEL.worker === "Workers", "section labels");
const counts = countBySection([
  { roleProfile: "planner" },
  { roleProfile: "worker" },
  { roleProfile: null },
  { roleProfile: "worker" },
]);
assert(counts.managers === 1 && counts.workers === 3, "counts split by section");
assert(summarizeGroupClose(counts) === "1 manager, 3 workers", "singular manager, plural workers");
assert(summarizeGroupClose({ managers: 0, workers: 2 }) === "2 workers", "omits zero managers");
assert(summarizeGroupClose({ managers: 2, workers: 0 }) === "2 managers", "omits zero workers");
assert(summarizeGroupClose({ managers: 1, workers: 1 }) === "1 manager, 1 worker", "both singular");

console.log("✓ orchestrationSections: role→section matrix, badges, partition, group-close summary");
