import assert from "node:assert/strict";
import { test } from "node:test";
import type { Tab } from "../../lib/types";
import { reducer } from "./reducer";
import type { ExtendedState } from "./tree";
import {
  shouldRefreshRemoteTabs, remoteTabsFromOrdinaryProjection,
  applyRemotePeerAgentStates, refreshAttachedRemoteAgentStates,
} from "./remoteTabProjection";

const local: Tab = { id: "local", pty_id: null, tab_type: "Terminal", title: "Local", cwd: null };
const remote: Tab = { ...local, id: "remote", remote_peer_id: "peer" };

test("confirmed local-only inventory needs no ordinary tab projection", () => {
  assert.equal(shouldRefreshRemoteTabs(false, { local }, {}), false);
});

test("paired peers and unknown inventory retain remote discovery", () => {
  assert.equal(shouldRefreshRemoteTabs(true, { local }), true);
  assert.equal(shouldRefreshRemoteTabs(null, { local }), true);
});

test("remote tabs and saved attachments refresh after peers disappear", () => {
  assert.equal(shouldRefreshRemoteTabs(false, { remote }), true);
  assert.equal(shouldRefreshRemoteTabs(false, { local }, { remote }), true);
});

const attachedTab = (id: string): Tab => ({
  id: `mesh:peer/${id}`, title: id, pty_id: `mesh:peer/${id}`, tab_type: "Terminal", cwd: null,
  workspace_id: "windows", remote_peer_id: "peer", remote_tab_id: id,
});

test("ordinary projection selects only remote tabs", () => {
  const attached = attachedTab("shared-tab");
  assert.deepEqual(Object.keys(remoteTabsFromOrdinaryProjection([local, attached])), [attached.id]);
});

test("relayed idle -> working -> idle reaches desktop projected tabs identically to web snapshots", () => {
  const attachment = { ...attachedTab("shared-tab"), agent_state: { state: "idle" as const } };
  const id = attachment.id;
  let projected: Record<string, Tab> = { [id]: attachment };
  let desktop: ExtendedState = {
    workspaces: [], tabs: projected, teams: [], notifications: [], tabStatuses: {},
    toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
    active_workspace_id: null, focused_tab_id: null,
  };
  for (const state of ["idle", "working", "idle"] as const) {
    const source: Tab = { ...local, id: "shared-tab", agent_state: { state, phase: state === "working" ? "tool" : null } };
    projected = applyRemotePeerAgentStates(projected, { peer_id: "peer", peer_name: "Mac", tabs: [source] });
    const web = remoteTabsFromOrdinaryProjection([{ ...attachment, agent_state: source.agent_state }]);
    desktop = reducer(desktop, { type: "SET_REMOTE_TABS", tabs: projected });
    assert.deepEqual(desktop.tabs[id].agent_state, web[id].agent_state);
    assert.equal(desktop.tabs[id].agent_state?.state, state);
    assert.equal(desktop.tabs[id].workspace_id, "windows");
    assert.equal(desktop.tabs[id].id, id);
  }
  assert.equal(attachment.agent_state.state, "idle");
});

test("attached refresh reads each owning peer once and skips ordinary-projected peers", async () => {
  const one = attachedTab("shared-tab");
  const two = attachedTab("second");
  const projected = { [one.id]: one, [two.id]: two };
  const reads: string[] = [];
  const refreshed = await refreshAttachedRemoteAgentStates(projected, {}, async (peerId) => {
    reads.push(peerId);
    return { peer_id: peerId, peer_name: "Mac", tabs: [
      { ...local, id: "shared-tab", agent_state: { state: "working" } },
      { ...local, id: "second", agent_state: { state: "idle" } },
      { ...local, id: "unattached", agent_state: { state: "working" } },
    ] };
  });
  assert.deepEqual(reads, ["peer"]);
  assert.equal(refreshed[one.id].agent_state?.state, "working");
  assert.equal(refreshed[two.id].agent_state?.state, "idle");
  assert.deepEqual(Object.keys(refreshed), Object.keys(projected));
  const ordinaryReads: string[] = [];
  await refreshAttachedRemoteAgentStates(refreshed, refreshed, async (peerId) => {
    ordinaryReads.push(peerId);
    throw new Error("Ordinary-projected tabs already have live state");
  });
  assert.deepEqual(ordinaryReads, []);
});

test("unreachable peers retain state and unrelated identities cannot change it", async () => {
  const one = { ...attachedTab("shared-tab"), agent_state: { state: "working" as const } };
  const projected = { [one.id]: one };
  const failed = await refreshAttachedRemoteAgentStates(projected, {}, async () => { throw new Error("offline"); });
  assert.deepEqual(failed, projected);
  assert.deepEqual(applyRemotePeerAgentStates(projected, {
    peer_id: "other", peer_name: "Other", tabs: [{ ...local, id: "shared-tab" }],
  }), projected);
  const cleared = applyRemotePeerAgentStates(projected, {
    peer_id: "peer", peer_name: "Mac", tabs: [{ ...local, id: "shared-tab", agent_state: null }],
  });
  assert.equal(cleared[one.id].agent_state, null);
});
