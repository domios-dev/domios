import { beforeEach, describe, expect, it, vi } from "vitest";
import { __setInvokeForTest } from "../../lib/bridge";
import { createTabsTransport } from "../../lib/transport/tabs";
import { projectAttachedRemoteTab } from "../../lib/remoteTabPortal";
import type { MultiplexerState, Tab } from "../../lib/types";
import { findObservedTabOwnerWorkspaceId, resolveRemoteWorkspaceForShortcut } from "../Sidebar/remoteWorkspaceManagement";
import { readTabCloseBlueprints } from "./blueprintOwner";

const mocks = vi.hoisted(() => ({
  peerJson: vi.fn(),
  daemonTransportForPeer: vi.fn(),
}));

vi.mock("../../lib/daemonBootstrap", () => ({
  daemonTransportForPeer: mocks.daemonTransportForPeer,
}));

function wirePeerMuxSnapshot(): MultiplexerState {
  // Real Rust /api/mux/state Tab records never serialize `workspace_id`; membership lives on workspaces/teams.
  const plugin: Tab = {
    id: "plugin-tab",
    pty_id: "pty-1",
    tab_type: "Terminal",
    title: "Plugin",
    cwd: "/srv/codeterm",
    agent_type: "claude",
  };
  const turing: Tab = {
    id: "turing-tab",
    pty_id: "pty-2",
    tab_type: "Terminal",
    title: "Turing",
    cwd: "/srv/general",
    agent_type: "claude",
    role: "Manager",
    role_profile: "general_agent",
  };
  return {
    workspaces: [
      { id: "ws-general", name: "General", protected: true, cwd: "/srv/general", tabs: [turing.id], active_tab_id: turing.id },
      { id: "ws-codeterm", name: "codeterm", cwd: "/srv/codeterm", tabs: [plugin.id], active_tab_id: plugin.id },
    ],
    tabs: { [turing.id]: turing, [plugin.id]: plugin },
    teams: [],
    active_workspace_id: "ws-codeterm",
    focused_tab_id: plugin.id,
    generation: 7,
  };
}

describe("readTabCloseBlueprints remote owner workspace resolution", () => {
  beforeEach(() => {
    mocks.peerJson.mockReset();
    mocks.daemonTransportForPeer.mockReset();
    mocks.daemonTransportForPeer.mockReturnValue({ json: mocks.peerJson });
    __setInvokeForTest(null);
  });

  it("derives workspace_id from /api/mux/state workspaces so remote close never fails with 'did not list this tab's workspace'", async () => {
    const snapshot = wirePeerMuxSnapshot();
    const transport = createTabsTransport({
      json: async <T,>() => ({
        ok: true,
        peer: { instance_id: "rog", instance_name: "ROG" },
        data: snapshot,
      }) as T,
    });
    __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "mesh_peer_tabs_list") return await transport.mesh_peer_tabs_list(args ?? {}) as T;
      throw new Error(`Unexpected command ${cmd}`);
    });
    mocks.peerJson.mockResolvedValueOnce({
      blueprints: [
        { id: "bp-1", name: "Nightly", state: "running", owner: { kind: "tab", tab_id: "plugin-tab" } },
      ],
    });

    const running = await readTabCloseBlueprints("local-viewer-ws", "mesh:rog/plugin-tab");
    expect(running).toEqual(["Nightly"]);
    expect(mocks.daemonTransportForPeer).toHaveBeenCalledWith("rog");
    expect(mocks.peerJson).toHaveBeenCalledWith(
      "GET",
      "/api/automation/graph/workspace?workspace_id=ws-codeterm",
      undefined,
      undefined,
    );
  });

  it("uses the known ownerWorkspaceId from the row without calling mesh_peer_tabs_list", async () => {
    const invokeSpy = vi.fn();
    __setInvokeForTest(invokeSpy as never);
    mocks.peerJson.mockResolvedValueOnce({
      blueprints: [
        { id: "bp-2", name: "Deploy", state: "running", owner: { kind: "tab", tab_id: "plugin-tab" } },
        { id: "bp-3", name: "Idle job", state: "idle", owner: { kind: "tab", tab_id: "plugin-tab" } },
      ],
    });

    const running = await readTabCloseBlueprints("local-viewer-ws", "mesh:rog/plugin-tab", "ws-codeterm");
    expect(running).toEqual(["Deploy"]);
    expect(invokeSpy).not.toHaveBeenCalled();
    expect(mocks.peerJson).toHaveBeenCalledWith(
      "GET",
      "/api/automation/graph/workspace?workspace_id=ws-codeterm",
      undefined,
      undefined,
    );
  });

  it("preserves remote_workspace_id on attached remote tabs so attached closes skip the list lookup", () => {
    const source: Tab = {
      id: "plugin-tab",
      pty_id: "pty-1",
      tab_type: "Terminal",
      title: "Plugin",
      cwd: "/srv/codeterm",
      workspace_id: "ws-codeterm",
    };
    const attached = projectAttachedRemoteTab(source, { peer_id: "rog", peer_name: "ROG" }, "local-general");
    expect(attached.workspace_id).toBe("local-general");
    expect(attached.remote_workspace_id).toBe("ws-codeterm");
  });

  it("resolves observed owner workspaces from both peerWorkspaces and generals inventory", () => {
    const snapshot = wirePeerMuxSnapshot();
    expect(findObservedTabOwnerWorkspaceId(
      "mesh:rog/plugin-tab",
      [{ peerId: "rog", workspace: snapshot.workspaces[1], teams: [], tabs: [{ viewerId: "mesh:rog/plugin-tab", tab: snapshot.tabs["plugin-tab"] }] }],
      [],
    )).toBe("ws-codeterm");
    expect(findObservedTabOwnerWorkspaceId(
      "mesh:rog/turing-tab",
      [],
      [{ peerId: "rog", name: "ROG", availability: "online", sessions: [{ viewerId: "mesh:rog/turing-tab", tab: { ...snapshot.tabs["turing-tab"], workspace_id: "ws-general" } }] }],
    )).toBe("ws-general");
    expect(resolveRemoteWorkspaceForShortcut(
      "rog",
      { remote_peer_id: "rog", remote_workspace_id: "ws-codeterm" },
      [{ peerId: "rog", workspace: snapshot.workspaces[1], teams: [], tabs: [] }],
    )?.workspace.id).toBe("ws-codeterm");
  });
});
