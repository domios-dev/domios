import { describe, expect, finish, it } from "../test/standaloneTest";
import { configureDaemonTransport } from "./daemonBootstrap";
import {
  FrontendCacheRepository,
  setFrontendCacheStorageForTests,
  type FrontendCacheChanges,
  type FrontendCacheCommit,
  type FrontendCacheManifest,
  type FrontendCacheRecord,
  type FrontendCacheSnapshot,
  type FrontendCacheStorage,
} from "./frontendCacheRecords";
import type { Tab } from "./types";
import { loadAttachedRemoteTabs, projectAttachedRemoteTab, reconcileAttachedRemoteTabs, saveAttachedRemoteTabs } from "./remoteTabPortal";

class FakeStorage implements FrontendCacheStorage {
  private readonly scopes = new Map<string, { manifest: FrontendCacheManifest; records: Map<string, FrontendCacheRecord> }>();
  private readGate?: { entered: () => void; done: () => void; wait: Promise<void> };
  private nextCommit?: () => void;
  recordWrites = 0;
  commits = 0;

  holdNextRead(): { entered: Promise<void>; finished: Promise<void>; release: () => void } {
    let entered!: () => void;
    let done!: () => void;
    let release!: () => void;
    const enteredPromise = new Promise<void>((resolve) => { entered = resolve; });
    const finished = new Promise<void>((resolve) => { done = resolve; });
    const wait = new Promise<void>((resolve) => { release = resolve; });
    this.readGate = { entered, done, wait };
    return { entered: enteredPromise, finished, release };
  }

  waitForCommit(): Promise<void> {
    return new Promise<void>((resolve) => { this.nextCommit = resolve; });
  }

  async read(scope: string): Promise<FrontendCacheSnapshot | null> {
    const gate = this.readGate;
    this.readGate = undefined;
    if (gate) { gate.entered(); await gate.wait; gate.done(); }
    const value = this.scopes.get(scope);
    return value ? structuredClone({ manifest: value.manifest, records: [...value.records.values()] }) : null;
  }

  async commit(scope: string, expectedRevision: number, manifest: FrontendCacheManifest, changes: FrontendCacheChanges): Promise<FrontendCacheCommit> {
    const current = this.scopes.get(scope);
    if ((current?.manifest.revision ?? 0) !== expectedRevision) return "conflict";
    const records = new Map(current?.records ?? []);
    for (const row of changes.deletes) records.delete(`${row.kind}\u0000${row.id}`);
    for (const row of changes.upserts) {
      records.set(`${row.kind}\u0000${row.id}`, structuredClone(row));
      this.recordWrites++;
    }
    if (records.size !== manifest.recordCount) throw new Error("incomplete attachment transaction");
    this.scopes.set(scope, { manifest: structuredClone(manifest), records });
    this.commits++;
    this.nextCommit?.();
    this.nextCommit = undefined;
    return "committed";
  }

  async clear(scope: string, expectedRevision?: number): Promise<boolean> {
    const current = this.scopes.get(scope);
    if (expectedRevision !== undefined && (current?.manifest.revision ?? 0) !== expectedRevision) return false;
    return this.scopes.delete(scope);
  }
}

function installStorageGlobals(): void {
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
  };
  Object.defineProperty(globalThis, "window", { configurable: true, value: {
    location: { protocol: "http:", host: "fixture.test", origin: "http://fixture.test" },
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  } });
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: storage });
  Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: storage });
}

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

  it("persists only structurally valid composite attachments", async () => {
    installStorageGlobals();
    setFrontendCacheStorageForTests(new FakeStorage());
    configureDaemonTransport({ origin: "http://fixture.test", token: "remote-test-structural" });
    const projected = projectAttachedRemoteTab(source({ kind: "pty" }, "pty"), { peer_id: "peer", peer_name: "Peer" }, "ws");
    saveAttachedRemoteTabs({ [projected.id]: projected, invalid: { ...projected, id: "local" } });
    await (await import("./remoteTabPortal")).hydrateAttachedRemoteTabs();
    expect(Object.keys(loadAttachedRemoteTabs())).toEqual(["mesh:peer/remote-tab"]);
  });

  it("writes only changed attachment records and merges a local edit made during hydration", async () => {
    installStorageGlobals();
    const storage = new FakeStorage();
    setFrontendCacheStorageForTests(storage);
    configureDaemonTransport({ origin: "http://fixture.test", token: "remote-test-records" });
    const hydrated = await import("./remoteTabPortal").then((module) => module.hydrateAttachedRemoteTabs());
    expect(hydrated).toEqual({});
    const first = projectAttachedRemoteTab(source({ kind: "pty" }, "pty"), { peer_id: "peer", peer_name: "Peer" }, "ws");
    const second = { ...first, id: "mesh:peer/other", remote_tab_id: "other" };
    const done = storage.waitForCommit();
    saveAttachedRemoteTabs({ [first.id]: first, [second.id]: second });
    await done;
    expect(storage.recordWrites).toBe(2);
    saveAttachedRemoteTabs({ [first.id]: { ...first }, [second.id]: { ...second } });
    expect(storage.commits).toBe(1);
    const changedDone = storage.waitForCommit();
    saveAttachedRemoteTabs({ [first.id]: { ...first, title: "Changed" }, [second.id]: second });
    await changedDone;
    expect(storage.recordWrites).toBe(3);

    setFrontendCacheStorageForTests(storage);
    configureDaemonTransport({ origin: "http://fixture.test", token: "remote-test-pending-edit" });
    const scope = (await import("./frontendCacheRecords")).frontendCacheScope("codeterm:remote-tab-attachments:v1");
    expect(scope).not.toBeNull();
    const cached = { ...first, workspace_id: "cached-workspace" };
    await new FrontendCacheRepository(storage).write(scope!, [{ kind: "attachment", id: cached.id, value: cached }], { schemaVersion: 1 });
    const readGate = storage.holdNextRead();
    const module = await import("./remoteTabPortal");
    const hydration = module.hydrateAttachedRemoteTabs();
    await readGate.entered;
    const local = { ...second, workspace_id: "local-workspace" };
    module.saveAttachedRemoteTabs({ [local.id]: local });
    readGate.release();
    await readGate.finished;
    const merged = await hydration;
    expect(Object.keys(merged).sort()).toEqual([cached.id, local.id].sort());
    expect(merged[local.id].workspace_id).toBe("local-workspace");
  });
});

finish();
