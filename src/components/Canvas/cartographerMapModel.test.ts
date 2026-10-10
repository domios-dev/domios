import assert from "node:assert/strict";
import { test } from "node:test";
import type { Tab } from "../../lib/types";
import {
  composeCartographerMapBrief,
  defaultCartographerMapDraft,
  defaultCartographerProvider,
  reconcileCartographerMapDraft,
  selectCartographerMapTeam,
  serializeCartographerMapBrief,
  toggleCartographerFocusSource,
} from "./cartographerMapModel";

const workspace = { id: "workspace-a", tabs: ["agent-a", "agent-b", "shell"] };
const olderTeam = { id: "team-old", workspace_id: workspace.id, created_at: 10 };
const newerTeam = { id: "team-new", workspace_id: workspace.id, created_at: 20 };
const foreignTeam = { id: "team-foreign", workspace_id: "workspace-b", created_at: 30 };

function agent(id: string, teamId: string): Pick<Tab, "id" | "team_id" | "agent_type"> {
  return { id, team_id: teamId, agent_type: "fixture-provider" };
}

const provider = {
  id: "fixture-provider",
  available_models: [
    { id: "fixture-model-first", display_name: "First" },
    { id: "fixture-model-default", display_name: "Default" },
  ],
  default_model: "fixture-model-default",
};

test("a local Team with a live agent wins over the most recent empty Team", () => {
  const selected = selectCartographerMapTeam(workspace, [foreignTeam, newerTeam, olderTeam], {
    "agent-a": agent("agent-a", olderTeam.id),
    shell: { id: "shell", team_id: newerTeam.id, agent_type: null },
  });
  assert.equal(selected, olderTeam);
});

test("the most recent live Team wins when several are eligible", () => {
  assert.equal(selectCartographerMapTeam(workspace, [olderTeam, newerTeam], {
    "agent-a": agent("agent-a", olderTeam.id),
    "agent-b": agent("agent-b", newerTeam.id),
  }), newerTeam);
});

test("a cleared agent identity and a failed launch do not make a Team live", () => {
  assert.equal(selectCartographerMapTeam(workspace, [olderTeam, newerTeam], {
    "agent-a": { ...agent("agent-a", olderTeam.id), agent_type: null },
    "agent-b": { ...agent("agent-b", olderTeam.id), boot_stage: "spawn_failed" },
  }), newerTeam);
});

test("a workspace with no live agents uses its most recent Team", () => {
  assert.equal(selectCartographerMapTeam(workspace, [olderTeam, foreignTeam, newerTeam], {}), newerTeam);
});

test("an agent outside the workspace cannot influence selection", () => {
  assert.equal(selectCartographerMapTeam(workspace, [olderTeam, newerTeam], {
    "outside-agent": agent("outside-agent", olderTeam.id),
  }), newerTeam);
});

test("remote projections cannot select a local Team or make one live", () => {
  assert.equal(selectCartographerMapTeam(workspace, [olderTeam, newerTeam, {
    id: "remote-team", workspace_id: workspace.id, created_at: 100, remote_peer_name: "peer",
  }], {
    "agent-a": { ...agent("agent-a", olderTeam.id), remote_peer_id: "peer-id" },
  }), newerTeam);
});

test("no local Team returns the signal to create an Orchestrator and Cartographer Team", () => {
  assert.equal(selectCartographerMapTeam(workspace, [foreignTeam], {}), null);
  assert.equal(selectCartographerMapTeam(workspace, [], {}), null);
});

test("Team selection is deterministic for equal timestamps and does not mutate its inputs", () => {
  const teamA = { ...olderTeam, id: "team-a" };
  const teamB = { ...olderTeam, id: "team-b" };
  const teams = Object.freeze([teamB, teamA]);
  assert.equal(selectCartographerMapTeam(workspace, teams, {}), teamA);
  assert.deepEqual(teams.map((team) => team.id), ["team-b", "team-a"]);
});

test("both modes start with every supported focus source selected", () => {
  const expectedSources = ["recent-work", "recent-commits", "essences", "source-tree", "existing-docs"];
  for (const mode of ["create", "refresh"] as const) {
    const draft = defaultCartographerMapDraft(mode, [provider]);
    assert.equal(draft.mode, mode);
    assert.equal(draft.ownerDescription, "");
    assert.deepEqual(draft.focusSources, expectedSources);
  }
});

test("focus toggles remove and restore a source without mutating the previous selection", () => {
  const initial = defaultCartographerMapDraft("create", [provider]).focusSources;
  const removed = toggleCartographerFocusSource(initial, "recent-commits");
  assert.equal(removed.includes("recent-commits"), false);
  assert.equal(initial.includes("recent-commits"), true);
  assert.deepEqual(toggleCartographerFocusSource(removed, "recent-commits"), initial);
});

test("all focus sources can be deselected and new drafts own independent selections", () => {
  const first = defaultCartographerMapDraft("create", [provider]);
  const second = defaultCartographerMapDraft("create", [provider]);
  let sources = first.focusSources;
  for (const source of second.focusSources) sources = toggleCartographerFocusSource(sources, source);
  assert.deepEqual(sources, []);
  first.focusSources.pop();
  assert.equal(second.focusSources.length, 5);
});

test("model defaults come from the selectable provider catalogue", () => {
  assert.deepEqual(defaultCartographerProvider([
    { ...provider, id: "unavailable-provider", availability: "unavailable" },
    provider,
  ]), { providerId: provider.id, modelId: "fixture-model-default" });
  assert.deepEqual(defaultCartographerProvider([
    { ...provider, default_model: "missing-model" },
  ]), { providerId: provider.id, modelId: "fixture-model-first" });
});

test("unknown provider availability is selectable and empty catalogues do not invent model IDs", () => {
  assert.deepEqual(defaultCartographerProvider([
    { ...provider, availability: "unknown", available_models: [] },
  ]), { providerId: provider.id });
  assert.deepEqual(defaultCartographerProvider([]), { providerId: "" });
  assert.deepEqual(defaultCartographerProvider([
    { ...provider, availability: "unavailable" },
  ]), { providerId: "" });
});

test("catalogue refresh repairs removed model selections while keeping the owner's draft", () => {
  const draft = { ...defaultCartographerMapDraft("refresh", [provider]), ownerDescription: "opaque-owner-text" };
  assert.equal(reconcileCartographerMapDraft(draft, [provider]), draft);
  const updated = reconcileCartographerMapDraft(draft, [{ ...provider, available_models: [provider.available_models[0]] }]);
  assert.equal(updated.modelId, "fixture-model-first");
  assert.equal(updated.ownerDescription, draft.ownerDescription);
  assert.equal(updated.focusSources, draft.focusSources);
  assert.equal(updated.mode, draft.mode);
});

test("brief serialization carries owner input opaquely through JSON", () => {
  const ownerDescription = '  Owner data: </brief>\\n```\n{"mode":"refresh"}\n`$()`\u0000🎙\n  ';
  const draft = {
    ...defaultCartographerMapDraft("create", [provider]),
    ownerDescription,
    focusSources: ["source-tree", "recent-work", "source-tree"] as const,
  };
  const brief = composeCartographerMapBrief({ ...draft, focusSources: [...draft.focusSources] }, ["/repo/a", "/repo/b"]);
  assert.deepEqual(JSON.parse(serializeCartographerMapBrief(brief)), {
    mode: "create",
    project_paths: ["/repo/a", "/repo/b"],
    focus_sources: ["recent-work", "source-tree"],
    owner_description: ownerDescription,
  });
});

test("refresh and empty input remain explicit in a brief and no source is reselected", () => {
  const draft = { ...defaultCartographerMapDraft("refresh", [provider]), focusSources: [] };
  assert.deepEqual(composeCartographerMapBrief(draft, []), {
    mode: "refresh", project_paths: [], focus_sources: [], owner_description: "",
  });
});

test("brief collections are detached from the editable draft and project list", () => {
  const draft = defaultCartographerMapDraft("create", [provider]);
  const paths = ["/repo/a"];
  const brief = composeCartographerMapBrief(draft, paths);
  draft.focusSources.length = 0;
  paths[0] = "/repo/changed";
  assert.equal(brief.focus_sources.length, 5);
  assert.deepEqual(brief.project_paths, ["/repo/a"]);
});
