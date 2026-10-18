import assert from "node:assert/strict";
import { test } from "node:test";
import { configureDaemonTransport } from "./daemonBootstrap";
import type {
  FrontendCacheChanges,
  FrontendCacheCommit,
  FrontendCacheManifest,
  FrontendCacheRecord,
  FrontendCacheSnapshot,
  FrontendCacheStorage,
} from "./frontendCacheRecords";
import type { MultiplexerState, Tab } from "./types";

class FakeStorage implements FrontendCacheStorage {
  private readonly rows = new Map<string, { manifest: FrontendCacheManifest; records: Map<string, FrontendCacheRecord> }>();
  recordWrites = 0;
  recordDeletes = 0;
  manifestWrites = 0;
  persistedBytes = 0;
  failWrites = false;

  async read(scope: string): Promise<FrontendCacheSnapshot | null> {
    const snapshot = this.rows.get(scope);
    return snapshot ? structuredClone({ manifest: snapshot.manifest, records: [...snapshot.records.values()] }) : null;
  }

  async commit(scope: string, expectedRevision: number, manifest: FrontendCacheManifest, changes: FrontendCacheChanges): Promise<FrontendCacheCommit> {
    if (this.failWrites) throw new Error("quota exceeded");
    const current = this.rows.get(scope);
    if ((current?.manifest.revision ?? 0) !== expectedRevision) return "conflict";
    const records = new Map(current?.records ?? []);
    for (const row of changes.deletes) {
      if (records.delete(`${row.kind}\u0000${row.id}`)) this.recordDeletes++;
    }
    for (const row of changes.upserts) {
      records.set(`${row.kind}\u0000${row.id}`, structuredClone(row));
      this.recordWrites++;
      this.persistedBytes += new TextEncoder().encode(JSON.stringify(row)).byteLength;
    }
    assert.equal(records.size, manifest.recordCount);
    this.manifestWrites++;
    this.rows.set(scope, { manifest: structuredClone(manifest), records });
    return "committed";
  }

  async clear(scope: string, expectedRevision?: number): Promise<boolean> {
    const current = this.rows.get(scope);
    if (expectedRevision !== undefined && (current?.manifest.revision ?? 0) !== expectedRevision) return false;
    return this.rows.delete(scope);
  }

  seed(scope: string, manifest: FrontendCacheManifest, records: FrontendCacheRecord[]): void {
    this.rows.set(scope, { manifest, records: new Map(records.map((row) => [`${row.kind}\u0000${row.id}`, row])) });
  }
}

const globals = globalThis as unknown as {
  window: { location: { protocol: string; host: string; origin: string } };
  localStorage: { getItem(key: string): string | null; setItem(key: string, value: string): void; removeItem(key: string): void };
  sessionStorage: { getItem(key: string): string | null; setItem(key: string, value: string): void; removeItem(key: string): void };
  __BUILD_HASH__?: string;
};
const values = new Map<string, string>();
const storage = {
  getItem: (key: string) => values.get(key) ?? null,
  setItem: (key: string, value: string) => values.set(key, value),
  removeItem: (key: string) => values.delete(key),
};
globals.window = { location: { protocol: "http:", host: "localhost:7686", origin: "http://localhost:7686" } };
globals.localStorage = storage;
globals.sessionStorage = storage;
globals.__BUILD_HASH__ = "build-1";
configureDaemonTransport({ origin: "http://localhost:7686", token: "account-a" });

function sampleState(tabCount: number): MultiplexerState & { activeTabIds: Record<string, string | null> } {
  const tabs: Record<string, Tab> = {};
  const tabIds = Array.from({ length: tabCount }, (_, index) => `tab-${index}`);
  for (const id of tabIds) {
    tabs[id] = {
      id,
      title: `terminal ${id}`,
      pty_id: id,
      cwd: "C:/workspace",
      tab_type: "Terminal",
      agent_session_id: `session-${id}`,
      description: "session preview ".repeat(32),
    } as unknown as Tab;
  }
  return {
    workspaces: [{ id: "workspace-a", name: "Workspace A", tabs: tabIds, active_tab_id: "tab-0", cwd: "C:/workspace" }],
    tabs,
    teams: [],
    active_workspace_id: "workspace-a",
    focused_tab_id: "tab-0",
    generation: 7,
    tab_statuses: Object.fromEntries(tabIds.map((id) => [id, "Working"])),
    activeTabIds: { "workspace-a": null },
  };
}

test("state cache persists one changed tab record and skips identical semantic refreshes", async () => {
  const { setFrontendCacheStorageForTests } = await import("./frontendCacheRecords");
  const fake = new FakeStorage();
  setFrontendCacheStorageForTests(fake);
  const { loadCachedStateAsync, loadCachedStateSync, saveStateSnapshot, clearStateSnapshot } = await import("./stateCache");
  const first = sampleState(72);
  const initial = await saveStateSnapshot(first);
  assert.equal(initial?.recordWrites, 146);
  assert.equal(loadCachedStateSync()?.tabs["tab-0"].title, "terminal tab-0");
  assert.equal((await loadCachedStateAsync())?.tab_statuses?.["tab-0"], "Working");
  assert.equal((await loadCachedStateAsync())?.workspaces[0].active_tab_id, null);
  const baselineBytes = fake.persistedBytes;

  const changed = sampleState(72);
  changed.tabs["tab-41"] = { ...changed.tabs["tab-41"], title: "changed terminal" };
  const next = await saveStateSnapshot(changed);
  assert.equal(next?.recordWrites, 1);
  assert.equal(next?.recordDeletes, 0);
  const changedBytes = fake.persistedBytes - baselineBytes;
  assert.ok(changedBytes < baselineBytes / 40);
  const writes = fake.recordWrites;
  const unchanged = await saveStateSnapshot(changed);
  assert.equal(unchanged?.recordWrites, 0);
  assert.equal(unchanged?.manifestWrites, 0);
  assert.equal(fake.recordWrites, writes);
  console.log(`synthetic 72-tab cache: 146 records/${baselineBytes} record bytes -> 1 record/${changedBytes} bytes -> 0 records/0 bytes on identical refresh`);

  fake.failWrites = true;
  const live = sampleState(72);
  live.tabs["tab-41"] = { ...live.tabs["tab-41"], title: "live despite quota" };
  await saveStateSnapshot(live);
  assert.equal(loadCachedStateSync()?.tabs["tab-41"].title, "live despite quota");
  fake.failWrites = false;
  await clearStateSnapshot();
  assert.equal(loadCachedStateSync(), null);
});

test("state cache refuses changed namespace, build and schema before hydration", async () => {
  const { frontendCacheScope, setFrontendCacheStorageForTests } = await import("./frontendCacheRecords");
  const fake = new FakeStorage();
  setFrontendCacheStorageForTests(fake);
  const { loadCachedStateAsync, saveStateSnapshot } = await import("./stateCache");
  await saveStateSnapshot(sampleState(1));
  const scope = frontendCacheScope("codeterm:state-cache");
  assert.ok(scope);

  configureDaemonTransport({ origin: "http://localhost:7686", token: "account-b" });
  assert.equal(await loadCachedStateAsync(), null);
  configureDaemonTransport({ origin: "http://localhost:7686", token: "account-a" });
  globals.window.location.host = "localhost:7687";
  globals.window.location.origin = "http://localhost:7687";
  configureDaemonTransport({ origin: "http://localhost:7687", token: "account-a" });
  assert.equal(await loadCachedStateAsync(), null);
  globals.window.location.host = "localhost:7686";
  globals.window.location.origin = "http://localhost:7686";
  configureDaemonTransport({ origin: "http://localhost:7686", token: "account-a" });
  storage.setItem("codeterm-web-relay-peer:http://localhost:7686", "peer-b");
  assert.equal(await loadCachedStateAsync(), null);
  storage.removeItem("codeterm-web-relay-peer:http://localhost:7686");

  globals.__BUILD_HASH__ = "build-2";
  assert.equal(await loadCachedStateAsync(), null);
  assert.equal(await fake.read(scope!), null);
  globals.__BUILD_HASH__ = "build-1";
  await saveStateSnapshot(sampleState(1));
  const snapshot = await fake.read(scope!);
  assert.ok(snapshot);
  fake.seed(scope!, {
    ...snapshot!.manifest,
    metadata: { ...snapshot!.manifest.metadata, schemaVersion: 999 },
  }, snapshot!.records);
  setFrontendCacheStorageForTests(fake);
  assert.equal(await loadCachedStateAsync(), null);
  assert.equal(await fake.read(scope!), null);
});
