import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Notice } from "../../lib/notices";
import type { GeneralAvailability } from "../../store/multiplexer/pairedWorkspaceProjection";
import {
  remoteWorkspaceCreatePort,
  useRemoteWorkspaceCreate,
  type RemoteWorkspaceCreatePort,
} from "./useRemoteWorkspaceCreate";
import {
  remoteTeamClosePort,
  useRemoteTeamClose,
  type RemoteTeamClosePort,
  type RemoteTeamCloseTarget,
} from "./useRemoteTeamClose";

const transportMocks = vi.hoisted(() => ({
  json: vi.fn(),
  daemonTransportForPeer: vi.fn(),
  transportListeners: new Set<(client: unknown) => void>(),
}));

vi.mock("../../lib/daemonBootstrap", () => ({
  daemonTransportForPeer: transportMocks.daemonTransportForPeer,
  onDaemonTransport: (listener: (client: unknown) => void) => {
    transportMocks.transportListeners.add(listener);
    listener({ origin: () => "http://127.0.0.1:7686" });
    return () => { transportMocks.transportListeners.delete(listener); };
  },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const teamTarget: RemoteTeamCloseTarget = {
  peerId: "peer-b",
  workspaceId: "ws-remote",
  teamId: "team-b",
  teamName: "Crew B",
  machineName: "Desk B",
  availability: "online",
  members: [{ tabId: "mesh:peer-b/tab-1", title: "Lead", role: "Manager", roleProfile: "orchestrator" }],
};

describe("remote workspace create and team close hooks", () => {
  beforeEach(() => {
    transportMocks.json.mockReset();
    transportMocks.daemonTransportForPeer.mockReset();
  });

  it("creates a workspace on the exact selected peer via mesh relay without activating remotely", async () => {
    transportMocks.daemonTransportForPeer.mockReturnValue({ json: transportMocks.json });
    transportMocks.json.mockResolvedValueOnce({ id: "ws-new", name: "alpha" });

    const created = await remoteWorkspaceCreatePort.createWorkspace("peer-b", "alpha", "/srv/alpha");
    expect(created).toEqual({ id: "ws-new", name: "alpha" });
    expect(transportMocks.daemonTransportForPeer).toHaveBeenCalledWith("peer-b");
    expect(transportMocks.json).toHaveBeenCalledWith(
      "POST",
      "/api/workspaces",
      { name: "alpha", cwd: "/srv/alpha", activate: false },
      { signal: undefined, callerTabId: undefined },
    );
  });

  it("refuses workspace create when peer is offline, signed out, or update-required without sending", async () => {
    const createWorkspace = vi.fn<RemoteWorkspaceCreatePort["createWorkspace"]>();
    const onCreated = vi.fn();
    let availability: GeneralAvailability = "offline";
    let hook!: ReturnType<typeof useRemoteWorkspaceCreate>;
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    function Harness() {
      hook = useRemoteWorkspaceCreate(() => availability, onCreated, { createWorkspace });
      return null;
    }
    try {
      await act(async () => root.render(<Harness />));
      for (const [state, label] of [
        ["offline", "offline"],
        ["signed-out", "signed out"],
        ["update-required", "update required"],
      ] as const) {
        availability = state;
        const result = await hook.create("peer-b", "/srv/alpha");
        expect(result).toEqual({ kind: "unavailable", peerId: "peer-b", reason: label });
      }
      expect(createWorkspace).not.toHaveBeenCalled();

      availability = "online";
      createWorkspace.mockRejectedValueOnce(new Error("Peer not authorized to POST /api/workspaces"));
      const refused = await hook.create("peer-b", "/srv/alpha");
      expect(refused).toEqual({
        kind: "failed",
        peerId: "peer-b",
        detail: "Peer not authorized to POST /api/workspaces",
      });
      expect(onCreated).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("reads Team close preview and sends POST /api/teams/<id>/close on the exact selected peer", async () => {
    const signal = new AbortController().signal;
    transportMocks.daemonTransportForPeer.mockReturnValue({
      json: transportMocks.json,
      currentToken: () => "tok-1",
    });
    transportMocks.json.mockResolvedValueOnce({
      installation_id: "peer-b",
      workspace_id: "ws-remote",
      workspace_name: "Remote",
      protected: false,
      agent_tabs: [],
      remote_members: [],
      crons: [],
      blueprints: [],
      warnings: [],
      teams: [{
        id: "team-b",
        name: "Crew B",
        remote: false,
        members: [{ tab_id: "tab-1", title: "Lead", role: "Manager", role_profile: "orchestrator", remote: false }],
        crons: ["Nightly"],
        blueprints: ["Ship"],
        tasks: [],
        open_attempts: [],
        lanes: [],
      }],
    });

    const preview = await remoteTeamClosePort.readInventory("peer-b", "ws-remote", "team-b", signal);
    expect(preview).toMatchObject({
      id: "team-b",
      name: "Crew B",
      crons: ["Nightly"],
      blueprints: ["Ship"],
    });
    expect(transportMocks.daemonTransportForPeer).toHaveBeenCalledWith("peer-b");
    expect(transportMocks.json).toHaveBeenCalledWith(
      "GET",
      "/api/workspaces/ws-remote/close-preview",
      undefined,
      { signal },
    );

    transportMocks.json.mockResolvedValueOnce({ inventory: { removed: [], kept: [], orphan_branches: [] } });
    const inventory = await remoteTeamClosePort.closeTeam("peer-b", "team-b");
    expect(inventory).toEqual({ removed: [], kept: [], orphan_branches: [] });
    expect(transportMocks.json).toHaveBeenLastCalledWith(
      "POST",
      "/api/teams/team-b/close",
      { force: true, confirm: "team-b", force_busy: true, caller_tab_id: undefined, session_binding_nonce: undefined },
    );
  });

  it("surfaces Team close refusals as visible notices and blocks closes when peer is unavailable", async () => {
    const notices: Notice[] = [];
    const onClosed = vi.fn();
    const port: RemoteTeamClosePort = {
      readInventory: vi.fn(async () => ({
        id: "team-b",
        name: "Crew B",
        members: teamTarget.members,
        crons: [],
        blueprints: [],
        lanes: [],
      })),
      closeTeam: vi.fn(async () => { throw new Error("Peer lacks authority to close Team"); }),
      teamExists: vi.fn(async () => true),
    };
    let availability: GeneralAvailability = "online";
    let hook!: ReturnType<typeof useRemoteTeamClose>;
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    function Harness() {
      hook = useRemoteTeamClose(() => availability, onClosed, notice => notices.push(notice), port);
      return null;
    }
    try {
      await act(async () => root.render(<Harness />));
      await act(async () => { await hook.request(teamTarget, () => true); });
      expect(hook.pending?.phase).toBe("ready");

      await act(async () => { await hook.confirm(); });
      expect(onClosed).not.toHaveBeenCalled();
      expect(notices).toHaveLength(1);
      expect(notices[0]).toMatchObject({
        title: "Team remains open",
        severity: "Error",
      });
      expect(notices[0].detail).toContain("Peer lacks authority to close Team");

      availability = "signed-out";
      await act(async () => { await hook.request({ ...teamTarget, availability: "signed-out" }, () => true); });
      expect(hook.pending).toBeNull();
      expect(port.readInventory).toHaveBeenCalledTimes(1);
      expect(notices[1]).toMatchObject({
        title: "Team close unavailable",
        severity: "Error",
      });
      expect(notices[1].detail).toContain("signed out");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
