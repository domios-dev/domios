import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invalidateWebRequestContext, WebApiError } from "../../lib/webApi";
import { useRemoteWorkspaceClose, remoteWorkspaceClosePort, type RemoteWorkspaceClosePort } from "./useRemoteWorkspaceClose";

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

const target = {
  peerId: "peer-b",
  workspaceId: "shared-id",
  workspaceName: "Project B",
  machineName: "Desk B",
  availability: "online" as const,
  protected: false,
};
const inventory = {
  workspaceName: "Project B",
  agentTitles: ["Worker B"],
  crons: ["Daily check"],
  blueprints: ["Deploy B"],
  teams: [],
};

function mount(port: RemoteWorkspaceClosePort, isOnline = true) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  let hook!: ReturnType<typeof useRemoteWorkspaceClose>;
  const onClosed = vi.fn();
  function Harness() {
    hook = useRemoteWorkspaceClose(() => isOnline ? "online" : "offline", onClosed, port);
    return null;
  }
  return { host, root, get hook() { return hook; }, onClosed, Harness };
}

describe("remote workspace close", () => {
  beforeEach(() => {
    transportMocks.json.mockReset();
    transportMocks.daemonTransportForPeer.mockReset();
  });

  it("reads one exact selected-peer workspace preview and validates its identity and mappings", async () => {
    const signal = new AbortController().signal;
    transportMocks.daemonTransportForPeer.mockReturnValue({
      json: transportMocks.json,
      currentToken: () => "tok-1",
    });
    transportMocks.json.mockResolvedValueOnce({
      installation_id: "peer-b",
      workspace_id: "shared-id",
      workspace_name: "Project B",
      protected: false,
      agent_tabs: [
        { tab_id: "member-1", title: "Team Member" },
        { tab_id: "tab-b", title: "Worker B" },
      ],
      remote_members: [],
      crons: ["Daily check"],
      blueprints: ["Deploy B"],
      teams: [{
        id: "team-b",
        name: "Team B",
        remote: false,
        members: [{ tab_id: "member-1", title: "Team Member", role: "Worker", role_profile: "worker", remote: false }],
        crons: ["Team cron"],
        blueprints: ["Team pipeline"],
        tasks: [{
          plan_id: "pl-1",
          plan_title: "Release Plan",
          task_id: "t-1",
          task_name: "Ship API",
          status: "in_progress",
          orphan: true,
          remote_attempt: false,
        }],
        open_attempts: [{
          plan_id: "pl-1",
          task_id: "t-1",
          attempt_id: "att-42",
          executor_tab_id: "member-1",
          remote: true,
        }],
        lanes: [{
          plan_id: "pl-1",
          task_id: "t-1",
          path: "/lanes/dirty",
          branch: "lane/dirty",
          state: "dirty",
          detail: "dirty worktree",
        }],
      }],
      warnings: [{
        kind: "dirty_lane",
        team_id: "team-b",
        plan_id: "pl-1",
        task_id: "t-1",
        detail: "dirty worktree",
      }],
    });

    const mapped = await remoteWorkspaceClosePort.readInventory("peer-b", "shared-id", signal);

    expect(transportMocks.daemonTransportForPeer).toHaveBeenCalledTimes(1);
    expect(transportMocks.daemonTransportForPeer).toHaveBeenCalledWith("peer-b");
    expect(transportMocks.json).toHaveBeenCalledTimes(1);
    expect(transportMocks.json).toHaveBeenCalledWith(
      "GET",
      "/api/workspaces/shared-id/close-preview",
      undefined,
      { signal },
    );
    expect(mapped).toMatchObject({
      workspaceName: "Project B",
      agentTitles: ["Worker B"],
      crons: ["Daily check"],
      blueprints: ["Deploy B"],
      warnings: ["dirty worktree"],
    });
    expect(mapped.teams[0].lanes).toEqual([
      "Release Plan: Ship API (orphan task)",
      "Open attempt att-42 · member-1 (remote)",
      "/lanes/dirty · lane/dirty · dirty · dirty worktree",
    ]);

    transportMocks.json.mockResolvedValueOnce({
      installation_id: "peer-c",
      workspace_id: "shared-id",
      workspace_name: "Project C",
      protected: false,
      agent_tabs: [],
      remote_members: [],
      crons: [],
      blueprints: [],
      teams: [],
      warnings: [],
    });
    await expect(remoteWorkspaceClosePort.readInventory("peer-b", "shared-id", signal)).rejects.toThrow(
      "The close preview does not match the selected machine and workspace.",
    );

    transportMocks.json.mockResolvedValueOnce({
      installation_id: "peer-b",
      workspace_id: "other-workspace",
      workspace_name: "Other",
      protected: false,
      agent_tabs: [],
      remote_members: [],
      crons: [],
      blueprints: [],
      teams: [],
      warnings: [],
    });
    await expect(remoteWorkspaceClosePort.readInventory("peer-b", "shared-id", signal)).rejects.toThrow(
      "The close preview does not match the selected machine and workspace.",
    );

    transportMocks.json.mockResolvedValueOnce({
      installation_id: "peer-b",
      workspace_id: "shared-id",
      workspace_name: "General",
      protected: true,
      agent_tabs: [],
      remote_members: [],
      crons: [],
      blueprints: [],
      teams: [],
      warnings: [],
    });
    await expect(remoteWorkspaceClosePort.readInventory("peer-b", "shared-id", signal)).rejects.toThrow(
      "The selected workspace is protected and cannot be closed.",
    );
  });

  it("posts one exact-workspace close via the selected peer transport and reads back status honestly", async () => {
    transportMocks.daemonTransportForPeer.mockReturnValue({ json: transportMocks.json });
    transportMocks.json.mockResolvedValueOnce({ ok: true });

    await remoteWorkspaceClosePort.closeWorkspace("peer-b", "shared-id");

    expect(transportMocks.daemonTransportForPeer).toHaveBeenCalledWith("peer-b");
    expect(transportMocks.json).toHaveBeenCalledTimes(1);
    expect(transportMocks.json).toHaveBeenCalledWith(
      "POST",
      "/api/workspaces/shared-id/close",
      { force: true, confirm: "shared-id" },
      { signal: undefined, callerTabId: undefined },
    );

    transportMocks.json.mockRejectedValueOnce(new WebApiError(404, "/api/workspaces/shared-id/close-preview", "Workspace not found"));
    await expect(remoteWorkspaceClosePort.workspaceExists("peer-b", "shared-id")).resolves.toBe(false);

    transportMocks.json.mockRejectedValueOnce(new WebApiError(413, "/api/workspaces/shared-id/close-preview", "Too large"));
    await expect(remoteWorkspaceClosePort.workspaceExists("peer-b", "shared-id")).resolves.toBe(true);

    transportMocks.json.mockRejectedValueOnce(new WebApiError(503, "/api/workspaces/shared-id/close-preview", "Unavailable"));
    await expect(remoteWorkspaceClosePort.workspaceExists("peer-b", "shared-id")).rejects.toThrow("503");

    transportMocks.json.mockResolvedValueOnce({ installation_id: "peer-c", workspace_id: "shared-id" });
    await expect(remoteWorkspaceClosePort.workspaceExists("peer-b", "shared-id")).rejects.toThrow(
      "The close preview does not match the selected machine and workspace.",
    );
  });

  it("invalidates in-flight previews when peer availability, request epoch, or daemon transport rotates", async () => {
    let resolveInventory!: (value: typeof inventory) => void;
    const readInventory = vi.fn<RemoteWorkspaceClosePort["readInventory"]>(() => new Promise(resolve => { resolveInventory = resolve; }));
    const closeWorkspace = vi.fn(async () => {});
    const port: RemoteWorkspaceClosePort = {
      readInventory,
      closeWorkspace,
      workspaceExists: vi.fn(async () => false),
    };
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    let online = true;
    let hook!: ReturnType<typeof useRemoteWorkspaceClose>;
    function Harness() {
      hook = useRemoteWorkspaceClose(() => online ? "online" : "offline", vi.fn(), port);
      return null;
    }
    try {
      await act(async () => root.render(<Harness />));

      let pendingReq!: Promise<Awaited<ReturnType<typeof hook.request>>>;
      await act(async () => { pendingReq = hook.request(target, () => true); });
      online = false;
      let outcome!: Awaited<ReturnType<typeof hook.request>>;
      await act(async () => {
        resolveInventory(inventory);
        outcome = await pendingReq;
      });
      expect(outcome.kind).toBe("stale");
      expect(hook.pending).toBeNull();

      online = true;
      await act(async () => { pendingReq = hook.request(target, () => true); });
      invalidateWebRequestContext();
      await act(async () => {
        resolveInventory(inventory);
        outcome = await pendingReq;
      });
      expect(outcome.kind).toBe("stale");
      expect(hook.pending).toBeNull();

      await act(async () => { pendingReq = hook.request(target, () => true); });
      await act(async () => {
        for (const listener of transportMocks.transportListeners) listener({ origin: () => "http://127.0.0.1:7686", rotated: true });
        resolveInventory(inventory);
        outcome = await pendingReq;
      });
      expect(outcome.kind).toBe("stale");
      expect(hook.pending).toBeNull();
      expect(closeWorkspace).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("confirms and reads back the exact peer/workspace, then prevents a second close", async () => {
    const closeWorkspace = vi.fn(async () => {});
    const workspaceExists = vi.fn(async (peerId: string, workspaceId: string) => {
      expect([peerId, workspaceId]).toEqual(["peer-b", "shared-id"]);
      return false;
    });
    const port: RemoteWorkspaceClosePort = {
      readInventory: vi.fn(async (peerId, workspaceId) => {
        expect([peerId, workspaceId]).toEqual(["peer-b", "shared-id"]);
        return inventory;
      }),
      closeWorkspace,
      workspaceExists,
    };
    const mounted = mount(port);
    try {
      await act(async () => mounted.root.render(<mounted.Harness />));
      let requested!: Awaited<ReturnType<typeof mounted.hook.request>>;
      await act(async () => { requested = await mounted.hook.request(target, () => true); });
      expect(requested.kind).toBe("ready");
      expect(mounted.hook.pending).toMatchObject({ peerId: "peer-b", workspaceId: "shared-id", machineName: "Desk B", agentTitles: ["Worker B"] });
      let result!: Awaited<ReturnType<typeof mounted.hook.confirm>>;
      await act(async () => { result = await mounted.hook.confirm(); });
      expect(result.kind).toBe("closed");
      expect(closeWorkspace).toHaveBeenCalledTimes(1);
      expect(closeWorkspace).toHaveBeenCalledWith("peer-b", "shared-id");
      expect(workspaceExists).toHaveBeenCalledTimes(1);
      expect(mounted.onClosed).toHaveBeenCalledWith("peer-b", "shared-id");
      await act(async () => { result = await mounted.hook.confirm(); });
      expect(result.kind).toBe("none");
      expect(closeWorkspace).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => mounted.root.unmount());
      mounted.host.remove();
    }
  });

  it("cancels without writing and ignores an inventory reply after selection moves", async () => {
    const closeWorkspace = vi.fn(async () => {});
    const readInventory = vi.fn<RemoteWorkspaceClosePort["readInventory"]>(async () => inventory);
    const port: RemoteWorkspaceClosePort = {
      readInventory,
      closeWorkspace,
      workspaceExists: vi.fn(async () => false),
    };
    const mounted = mount(port);
    try {
      await act(async () => mounted.root.render(<mounted.Harness />));
      await act(async () => { await mounted.hook.request(target, () => true); });
      await act(async () => mounted.hook.cancel());
      expect(mounted.hook.pending).toBeNull();
      expect((await mounted.hook.confirm()).kind).toBe("none");
      expect(closeWorkspace).not.toHaveBeenCalled();

      let rejectInventory!: (error: Error) => void;
      readInventory.mockImplementationOnce(() => new Promise<Awaited<ReturnType<RemoteWorkspaceClosePort["readInventory"]>>>(
        (_, reject) => { rejectInventory = reject; },
      ));
      let selected = true;
      let request!: Promise<Awaited<ReturnType<typeof mounted.hook.request>>>;
      await act(async () => { request = mounted.hook.request(target, () => selected); });
      selected = false;
      let staleResult!: Awaited<ReturnType<typeof mounted.hook.request>>;
      await act(async () => {
        rejectInventory(new Error("late peer failure"));
        staleResult = await request;
      });
      expect(staleResult.kind).toBe("stale");
      expect(mounted.hook.pending).toBeNull();
      expect(closeWorkspace).not.toHaveBeenCalled();
    } finally {
      await act(async () => mounted.root.unmount());
      mounted.host.remove();
    }
  });

  it("keeps refusal and uncertain readback honest without replaying the close", async () => {
    const closeWorkspace = vi.fn(async () => { throw new Error("authority refused close"); });
    const port: RemoteWorkspaceClosePort = {
      readInventory: vi.fn(async () => inventory),
      closeWorkspace,
      workspaceExists: vi.fn(async () => true),
    };
    const mounted = mount(port);
    try {
      await act(async () => mounted.root.render(<mounted.Harness />));
      await act(async () => { await mounted.hook.request(target, () => true); });
      let result!: Awaited<ReturnType<typeof mounted.hook.confirm>>;
      await act(async () => { result = await mounted.hook.confirm(); });
      expect(result).toMatchObject({ kind: "still_present", detail: "authority refused close" });
      expect(mounted.onClosed).not.toHaveBeenCalled();
      expect(closeWorkspace).toHaveBeenCalledTimes(1);
      expect((await mounted.hook.confirm()).kind).toBe("none");
      expect(closeWorkspace).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => mounted.root.unmount());
      mounted.host.remove();
    }
  });

  it("does not report success or retry when the post-close readback is uncertain", async () => {
    const closeWorkspace = vi.fn(async () => { throw new Error("request outcome unknown"); });
    const port: RemoteWorkspaceClosePort = {
      readInventory: vi.fn(async () => inventory),
      closeWorkspace,
      workspaceExists: vi.fn(async () => { throw new Error("peer unavailable"); }),
    };
    const mounted = mount(port);
    try {
      await act(async () => mounted.root.render(<mounted.Harness />));
      await act(async () => { await mounted.hook.request(target, () => true); });
      let result!: Awaited<ReturnType<typeof mounted.hook.confirm>>;
      await act(async () => { result = await mounted.hook.confirm(); });
      expect(result.kind).toBe("unknown");
      expect(mounted.onClosed).not.toHaveBeenCalled();
      expect(closeWorkspace).toHaveBeenCalledTimes(1);
      expect((await mounted.hook.confirm()).kind).toBe("none");
      expect(closeWorkspace).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => mounted.root.unmount());
      mounted.host.remove();
    }
  });

  it("keeps a refused inventory from opening confirmation", async () => {
    const port: RemoteWorkspaceClosePort = {
      readInventory: vi.fn(async () => { throw new Error("authority revoked"); }),
      closeWorkspace: vi.fn(async () => {}),
      workspaceExists: vi.fn(async () => false),
    };
    const mounted = mount(port);
    try {
      await act(async () => mounted.root.render(<mounted.Harness />));
      let result!: Awaited<ReturnType<typeof mounted.hook.request>>;
      await act(async () => { result = await mounted.hook.request(target, () => true); });
      expect(result).toMatchObject({ kind: "inventory_failed", detail: "authority revoked" });
      expect(mounted.hook.pending).toBeNull();
      expect(port.closeWorkspace).not.toHaveBeenCalled();
    } finally {
      await act(async () => mounted.root.unmount());
      mounted.host.remove();
    }
  });

  it("does not inspect or close an offline peer", async () => {
    const port: RemoteWorkspaceClosePort = {
      readInventory: vi.fn(async () => inventory),
      closeWorkspace: vi.fn(async () => {}),
      workspaceExists: vi.fn(async () => false),
    };
    const mounted = mount(port, false);
    try {
      await act(async () => mounted.root.render(<mounted.Harness />));
      let result!: Awaited<ReturnType<typeof mounted.hook.request>>;
      await act(async () => { result = await mounted.hook.request(target, () => true); });
      expect(result.kind).toBe("offline");
      expect(port.readInventory).not.toHaveBeenCalled();
      expect(port.closeWorkspace).not.toHaveBeenCalled();
    } finally {
      await act(async () => mounted.root.unmount());
      mounted.host.remove();
    }
  });
});
