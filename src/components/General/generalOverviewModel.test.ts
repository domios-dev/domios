import assert from "node:assert/strict";
import { generalInventorySections, generalWorkspaceJobs, projectUtilityShells } from "./generalOverviewModel";
import { daemon, owner, schedule, shell, tabs, teams, workspaces } from "./generalOverview.testdata";

const localDaemon = { ...daemon, source_peer_id: undefined, source_peer: undefined };
const sections = generalInventorySections({ resources: [localDaemon, daemon, schedule] }, "Local");
assert.deepEqual(sections.services.rows.map((row) => row.resource), [localDaemon]);
assert.deepEqual(generalInventorySections({ resources: [daemon, schedule] }, "Local").services.rows, []);
assert.deepEqual(generalInventorySections({ resources: [] }, "Local").services.rows, []);
assert.equal(sections.services.rows[0].freshness, "offline");
assert.equal(sections.services.rows[0].resource.capabilities?.delete?.unavailable_reason, "offline");
console.log("General includes local daemon registry only, preserving availability");

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

assert.equal(projectUtilityShells(workspaces, tabs, teams, "other").length, 0);
assert.equal(projectUtilityShells(workspaces, tabs, teams, "project").length, 1);
assert.equal(projectUtilityShells(workspaces, { ...tabs, shell: { ...shell, remote_peer_id: "remote" } }, teams, "project").length, 0);
const generalJob = { kind: "cron" as const, cron: { workspace_id: "general" } as import("../../lib/types").CronJob, title: "General", active: true, state: "Enabled" };
const projectJob = { ...generalJob, cron: { ...generalJob.cron, workspace_id: "project" } };
assert.deepEqual(generalWorkspaceJobs([generalJob, projectJob], "general"), [generalJob]);
assert.deepEqual(generalWorkspaceJobs([projectJob], "general"), []);
console.log("General scope excludes foreign workspaces and remote shell proxies");
