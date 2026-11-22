// Run: npx tsx src/store/multiplexer/usePairedWorkspaces.test.ts
import assert from "node:assert/strict";
import { needsPeerPlanTitlesFetch, unnamedTeamIdsForWorkspaces, type CachedPlanTitles } from "./usePairedWorkspaces";
import type { BrowsedWorkspace } from "./pairedWorkspaceProjection";

function makeWorkspace(teamId: string, teamName?: string): BrowsedWorkspace {
  return {
    id: "ws-1",
    name: "Workspace",
    peerId: "peer-a",
    teams: teamName !== undefined ? [{ id: teamId, name: teamName }] : [],
    tabs: [{ tab: { id: "tab-1", team_id: teamId } as never, kind: "agent" as never }],
  } as unknown as BrowsedWorkspace;
}

// 1. Unnamed team requires a lookup once, then subsequent refreshes reuse cached titles without refetching.
const unnamed = unnamedTeamIdsForWorkspaces([makeWorkspace("team_abc")]);
assert.deepEqual(unnamed, ["team_abc"]);
assert.equal(needsPeerPlanTitlesFetch(unnamed, undefined, "pk-1"), true);

const cached: CachedPlanTitles = {
  publicKey: "pk-1",
  planTitles: new Map([["team_abc", "Domios 1.21.2"]]),
  lookedUpTeamIds: new Set(["team_abc"]),
};
assert.equal(needsPeerPlanTitlesFetch(unnamed, cached, "pk-1"), false);

// 2. A newly appearing unnamed team triggers a single new lookup.
const withSecondTeam = unnamedTeamIdsForWorkspaces([
  makeWorkspace("team_abc"),
  makeWorkspace("team_xyz"),
]);
assert.deepEqual(withSecondTeam.sort(), ["team_abc", "team_xyz"]);
assert.equal(needsPeerPlanTitlesFetch(withSecondTeam, cached, "pk-1"), true);

// 3. Peer public key rotation invalidates the cache.
assert.equal(needsPeerPlanTitlesFetch(unnamed, cached, "pk-2"), true);

// 4. Named teams never request /api/plans at all.
const named = unnamedTeamIdsForWorkspaces([makeWorkspace("team_abc", "Domios 1.21.2")]);
assert.deepEqual(named, []);
assert.equal(needsPeerPlanTitlesFetch(named, undefined, "pk-1"), false);

console.log("usePairedWorkspaces.test.ts: PASS");
