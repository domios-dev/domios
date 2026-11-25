import assert from "node:assert/strict";
import { test } from "node:test";
import type { Tab } from "../../lib/types";
import { tabActivity } from "../../lib/tabActivity";
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
  assert.equal(refreshed.tabs[one.id].agent_state?.state, "working");
  assert.equal(refreshed.tabs[two.id].agent_state?.state, "idle");
  assert.deepEqual(Object.keys(refreshed.tabs), Object.keys(projected));
  const ordinaryReads: string[] = [];
  await refreshAttachedRemoteAgentStates(refreshed.tabs, refreshed.tabs, async (peerId) => {
    ordinaryReads.push(peerId);
    throw new Error("Ordinary-projected tabs already have live state");
  });
  assert.deepEqual(ordinaryReads, []);
});

test("unreachable peers retain state and unrelated identities cannot change it", async () => {
  const one = { ...attachedTab("shared-tab"), agent_state: { state: "working" as const } };
  const projected = { [one.id]: one };
  const failed = await refreshAttachedRemoteAgentStates(projected, {}, async () => { throw new Error("offline"); });
  assert.deepEqual(failed.tabs, projected);
  assert.deepEqual(failed.statuses, {});
  assert.deepEqual(applyRemotePeerAgentStates(projected, {
    peer_id: "other", peer_name: "Other", tabs: [{ ...local, id: "shared-tab" }],
  }), projected);
  const cleared = applyRemotePeerAgentStates(projected, {
    peer_id: "peer", peer_name: "Mac", tabs: [{ ...local, id: "shared-tab", agent_state: null }],
  });
  assert.equal(cleared[one.id].agent_state, null);
});

test("an attached chat projects its peer status into mux activity and clears on authoritative idle", async () => {
  const id = "mesh:peer/shared-chat";
  const attachment: Tab = {
    ...attachedTab("shared-chat"),
    agent_type: "acp",
    agent_session_id: "peer-session",
    backing: { kind: "chat_backend", plugin_id: "acp", session_id: "peer-session" },
    agent_state: { state: "idle" },
  };
  const projected = { [id]: attachment };
  const working = await refreshAttachedRemoteAgentStates(projected, {}, async () => ({
    peer_id: "peer",
    peer_name: "Mac",
    tabs: [{ ...attachment, id: "shared-chat", remote_peer_id: undefined, remote_tab_id: undefined,
      agent_state: { state: "working", phase: "tool" } }],
    tab_statuses: { "shared-chat": "Working" },
  }));
  assert.equal(working.statuses[id], "Working");
  const workingState: ExtendedState = {
    workspaces: [], tabs: {}, teams: [], notifications: [], tabStatuses: {},
    toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
    active_workspace_id: null, focused_tab_id: null,
  };
  const active = reducer(workingState, {
    type: "SET_REMOTE_TABS", tabs: working.tabs, remoteStatuses: working.statuses,
  });
  assert.equal(active.tabStatuses[id], "Working");
  assert.equal(tabActivity(active.tabs[id], active.tabStatuses[id]).canStop, true);

  const idle = await refreshAttachedRemoteAgentStates(working.tabs, {}, async () => ({
    peer_id: "peer",
    peer_name: "Mac",
    tabs: [{ ...attachment, id: "shared-chat", remote_peer_id: undefined, remote_tab_id: undefined,
      agent_state: { state: "idle", status_line: "tool_call pending" } }],
    tab_statuses: { "shared-chat": "Idle" },
  }));
  const settled = reducer(active, { type: "SET_REMOTE_TABS", tabs: idle.tabs, remoteStatuses: idle.statuses });
  assert.equal(settled.tabStatuses[id], "Idle");
  assert.equal(tabActivity(settled.tabs[id], settled.tabStatuses[id]).canStop, false);
});

test("a peer PTY that appears after the attach reaches the attachment, and a gone one clears it", () => {
  const shell = { ...attachedTab("shell"), pty_id: null };
  const appeared = applyRemotePeerAgentStates({ [shell.id]: shell }, {
    peer_id: "peer", peer_name: "Mac", tabs: [{ ...local, id: "shell", pty_id: "peer-pty" }],
  });
  assert.equal(appeared[shell.id].pty_id, shell.id);
  assert.equal(appeared[shell.id].remote_pty_id, "peer-pty");
  const gone = applyRemotePeerAgentStates(appeared, {
    peer_id: "peer", peer_name: "Mac", tabs: [{ ...local, id: "shell", pty_id: null }],
  });
  assert.equal(gone[shell.id].pty_id, null);
  assert.equal(gone[shell.id].remote_pty_id, null);
});

test("a manager attached while launching follows its peer's session, keeping this machine's placement and channels", () => {
  const launching: Tab = { ...attachedTab("lead"), agent_session_id: null, boot_stage: "agent_launching",
    remote_channels: { control: "reconnecting", output: "unavailable", chat: "available" } };
  const ready = applyRemotePeerAgentStates({ [launching.id]: launching }, { peer_id: "peer", peer_name: "Mac", tabs: [
    { ...local, id: "lead", title: "Lead", agent_session_id: "SESSION-FRAGMENT", boot_stage: "agent_ready", workspace_id: "peer-workspace" },
  ] });
  const tab = ready[launching.id];
  assert.equal(tab.agent_session_id, "SESSION-FRAGMENT");
  assert.equal(tab.boot_stage, "agent_ready");
  assert.equal(tab.workspace_id, "windows");
  assert.equal(tab.remote_channels?.control, "available");
  assert.equal(tab.remote_channels?.output, "unavailable");
  assert.equal(tab.id, launching.id);
});

test("an attachment the answering peer no longer lists leaves; an unreachable peer keeps it", async () => {
  const kept = attachedTab("kept");
  const closed = attachedTab("closed");
  const projected = { [kept.id]: kept, [closed.id]: closed };
  const refreshed = await refreshAttachedRemoteAgentStates(projected, {}, async (peerId) => ({
    peer_id: peerId, peer_name: "Mac", tabs: [{ ...local, id: "kept", pty_id: "pty" }],
  }));
  assert.deepEqual(Object.keys(refreshed.tabs), [kept.id]);
  const offline = await refreshAttachedRemoteAgentStates(projected, {}, async () => { throw new Error("offline"); });
  assert.deepEqual(Object.keys(offline.tabs).sort(), [closed.id, kept.id].sort());
});
