import assert from "node:assert/strict";
import { generalInventorySections, projectUtilityShells } from "./generalOverviewModel";
import { daemon, owner, schedule, shell, tabs, teams, workspaces } from "./generalOverview.testdata";

const sections = generalInventorySections({ resources: [daemon, schedule] }, "Local");
assert.deepEqual(sections.services.rows.map((row) => row.resource), [daemon]);
assert.deepEqual(sections.schedules.rows.map((row) => row.resource), [schedule]);
assert.equal(sections.services.rows[0].machineName, daemon.machine_name);
assert.equal(sections.services.rows[0].freshness, "offline");
assert.equal(sections.services.rows[0].resource.capabilities?.delete?.unavailable_reason, "offline");
const staleDaemon = { ...daemon, provenance: { freshness: "stale" as const, offline: false }, capabilities: { delete: { allowed: false, unavailable_reason: "stale" as const } } };
const stale = generalInventorySections({ resources: [staleDaemon] }, "Local").services.rows[0];
assert.equal(stale.freshness, "stale");
assert.equal(stale.resource.capabilities?.delete?.unavailable_reason, "stale");
console.log("✓ General rail sections own daemon and cron inventory separately with provenance intact");

const before = structuredClone({ workspaces, tabs, teams });
const [row] = projectUtilityShells(workspaces, tabs, teams);
assert.equal(row.tab, shell);
assert.deepEqual(row.owner, { id: owner.id, name: owner.title });
assert.deepEqual(row.team, { id: teams[0].id, name: teams[0].name });
assert.deepEqual(row.workspace, { id: workspaces[0].id, name: workspaces[0].name });
assert.deepEqual(row.target, { workspaceId: workspaces[0].id, tabId: shell.id });
assert.deepEqual({ workspaces, tabs, teams }, before);
console.log("✓ Utility shell projection retains owner, Team, workspace and navigation without mutation");

for (const excluded of [owner, { ...shell, role: "Worker" as const }, { ...shell, roleless_tracked: true }, { ...shell, tab_type: "Choice" as const }, { ...shell, backing: { kind: "chat_backend" as const, plugin_id: "fixture", session_id: "s" } }]) {
  assert.deepEqual(projectUtilityShells(workspaces, { ...tabs, shell: { ...excluded, id: shell.id }, owner }, teams), []);
}
assert.deepEqual(projectUtilityShells(workspaces, { owner }, teams), []);
const [orphan] = projectUtilityShells(workspaces, { shell: { ...shell, parent_tab_id: "closed" } }, []);
assert.deepEqual(orphan.owner, { id: "closed", name: "closed" });
assert.deepEqual(orphan.team, { id: "team", name: "team" });
console.log("✓ Projection follows isUtilityShell and keeps missing-owner identity without inventing a target");
