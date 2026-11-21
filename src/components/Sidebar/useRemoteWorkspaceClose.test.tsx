import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { useRemoteWorkspaceClose, remoteWorkspaceClosePort, type RemoteWorkspaceClosePort } from "./useRemoteWorkspaceClose";

const transportMocks = vi.hoisted(() => ({
  json: vi.fn(),
  daemonTransportForPeer: vi.fn(),
}));

vi.mock("../../lib/daemonBootstrap", () => ({
  daemonTransportForPeer: transportMocks.daemonTransportForPeer,
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
  it("reads one exact selected-peer workspace preview and validates its identity", async () => {
    const signal = new AbortController().signal;
    transportMocks.daemonTransportForPeer.mockReturnValue({ json: transportMocks.json });
    transportMocks.json.mockResolvedValueOnce({
      installation_id: "peer-b",
      workspace_id: "shared-id",
      workspace_name: "Project B",
      protected: false,
      agent_tabs: [{ tab_id: "tab-b", title: "Worker B" }],
      remote_members: [],
      crons: ["Daily check"],
      blueprints: ["Deploy B"],
      teams: [],
      warnings: [],
    });

    const inventory = await remoteWorkspaceClosePort.readInventory("peer-b", "shared-id", signal);

    expect(transportMocks.daemonTransportForPeer).toHaveBeenCalledTimes(1);
    expect(transportMocks.daemonTransportForPeer).toHaveBeenCalledWith("peer-b");
    expect(transportMocks.json).toHaveBeenCalledTimes(1);
    expect(transportMocks.json).toHaveBeenCalledWith(
      "GET",
      "/api/workspaces/shared-id/close-preview",
      undefined,
      { signal },
    );
    expect(inventory).toMatchObject({ workspaceName: "Project B", agentTitles: ["Worker B"], blueprints: ["Deploy B"] });

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
