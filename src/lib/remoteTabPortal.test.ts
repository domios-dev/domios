import { describe, expect, finish, it } from "../test/standaloneTest";
import type { Tab } from "./types";
import { loadAttachedRemoteTabs, projectAttachedRemoteTab, reconcileAttachedRemoteTabs, saveAttachedRemoteTabs } from "./remoteTabPortal";

const source = (backing: Tab["backing"], ptyId: string | null): Tab => ({
  id: "remote-tab",
  pty_id: ptyId,
  backing,
  tab_type: "Terminal",
  title: "Remote work",
  cwd: null,
});

describe("remote Tab portal projection", () => {
  it("refreshes live session and activity while preserving the attachment workspace", () => {
    const attached = projectAttachedRemoteTab(source({ kind: "pty" }, "pty"), { peer_id: "peer", peer_name: "Peer" }, "local-ws");
    const live = { ...attached, workspace_id: "remote-ws", agent_session_id: "new-session", agent_state: { state: "working" as const, elapsed_ms: 8000 } };
    const result = reconcileAttachedRemoteTabs({ [live.id]: live }, { [attached.id]: attached });
    expect(result[live.id].agent_state).toEqual(live.agent_state);
    expect(result[live.id].agent_session_id).toBe("new-session");
    expect(result[live.id].workspace_id).toBe("local-ws");
  });

  it("keeps unattached live tabs and disconnected attachments", () => {
    const attached = projectAttachedRemoteTab(source({ kind: "pty" }, "pty"), { peer_id: "peer", peer_name: "Peer" }, "ws");
    const live = { ...attached, id: "mesh:peer/other", remote_tab_id: "other" };
    const result = reconcileAttachedRemoteTabs({ [live.id]: live }, { [attached.id]: attached });
    expect(result[live.id]).toEqual(live);
    expect(result[attached.id]).toEqual(attached);
  });
  it("uses composite route identity without rewriting backing capabilities", () => {
    const projected = projectAttachedRemoteTab(
      source({ kind: "pty" }, "remote-pty"),
      { peer_id: "peer-a", peer_name: "Build box" },
      "workspace-a",
    );
    expect(projected.id).toBe("mesh:peer-a/remote-tab");
    expect(projected.pty_id).toBe("mesh:peer-a/remote-tab");
    expect(projected.backing).toEqual({ kind: "pty" });
    expect(projected.remote_tab_id).toBe("remote-tab");
    expect(projected.workspace_id).toBe("workspace-a");
  });

  it("keeps non-PTY backing detached from a fake PTY identity", () => {
    const projected = projectAttachedRemoteTab(
      source({ kind: "chat_backend", plugin_id: "local-model", session_id: "session-a" }, null),
      { peer_id: "peer-a", peer_name: "Build box" },
      "workspace-a",
    );
    expect(projected.pty_id).toBeNull();
    expect(projected.backing).toEqual({ kind: "chat_backend", plugin_id: "local-model", session_id: "session-a" });
  });

  it("persists only structurally valid composite attachments", () => {
    const values = new Map<string, string>();
    Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    } });
    const projected = projectAttachedRemoteTab(source({ kind: "pty" }, "pty"), { peer_id: "peer", peer_name: "Peer" }, "ws");
    saveAttachedRemoteTabs({ [projected.id]: projected, invalid: { ...projected, id: "local" } });
    expect(Object.keys(loadAttachedRemoteTabs())).toEqual(["mesh:peer/remote-tab"]);
  });
});

finish();
