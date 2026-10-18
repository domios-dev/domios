import assert from "node:assert/strict";
import { test } from "node:test";
import {
  FrontendCacheRepository,
  type FrontendCacheChanges,
  type FrontendCacheCommit,
  type FrontendCacheManifest,
  type FrontendCacheRecord,
  type FrontendCacheSnapshot,
  type FrontendCacheStorage,
} from "./frontendCacheRecords";

class FakeStorage implements FrontendCacheStorage {
  private readonly scopes = new Map<string, { manifest: FrontendCacheManifest; records: Map<string, FrontendCacheRecord> }>();
  private blocked?: { entered: () => void; wait: Promise<void> };
  failWrites = false;
  recordWrites = 0;
  recordDeletes = 0;
  manifestWrites = 0;
  persistedBytes = 0;

  holdNextCommit(): { entered: Promise<void>; release: () => void } {
    let entered!: () => void;
    let release!: () => void;
    const enteredPromise = new Promise<void>((resolve) => { entered = resolve; });
    const wait = new Promise<void>((resolve) => { release = resolve; });
    this.blocked = { entered, wait };
    return { entered: enteredPromise, release };
  }

  async read(scope: string): Promise<FrontendCacheSnapshot | null> {
    const value = this.scopes.get(scope);
    if (!value) return null;
    return structuredClone({ manifest: value.manifest, records: [...value.records.values()] });
  }

  async commit(
    scope: string,
    expectedRevision: number,
    manifest: FrontendCacheManifest,
    changes: FrontendCacheChanges,
  ): Promise<FrontendCacheCommit> {
    const blocked = this.blocked;
    this.blocked = undefined;
    if (blocked) {
      blocked.entered();
      await blocked.wait;
    }
    if (this.failWrites) throw new Error("quota exceeded");
    const current = this.scopes.get(scope);
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
    assert.equal(records.size, manifest.recordCount, "the fake transaction commits a complete record generation");
    this.manifestWrites++;
    this.scopes.set(scope, { manifest: structuredClone(manifest), records });
    return "committed";
  }

  async clear(scope: string, expectedRevision?: number): Promise<boolean> {
    const current = this.scopes.get(scope);
    if (expectedRevision !== undefined && (current?.manifest.revision ?? 0) !== expectedRevision) return false;
    return this.scopes.delete(scope);
  }
}

function largeRows(count: number): FrontendCacheRecord[] {
  return Array.from({ length: count }, (_, index) => ({
    kind: "tab",
    id: `tab-${index}`,
    value: {
      id: `tab-${index}`,
      title: `terminal-${index}`,
      session: `session-${index}`,
      preview: "0123456789abcdef".repeat(48),
    },
  }));
}

test("incremental cache commits write changed records only and skip identical refreshes", async () => {
  const storage = new FakeStorage();
  const cache = new FrontendCacheRepository(storage);
  const first = largeRows(96);
  const baselineBytes = first.reduce((total, record) => total + new TextEncoder().encode(JSON.stringify(record)).byteLength, 0);
  const initial = await cache.write("host-a/account-a/self", first, { generation: 4 });
  assert.equal(initial.recordWrites, 96);
  assert.equal(initial.manifestWrites, 1);
  assert.equal(storage.persistedBytes, baselineBytes);

  const next = first.map((record, index) => index === 41
    ? { ...record, value: { ...(record.value as object), title: "changed terminal" } }
    : record);
  const beforeBytes = storage.persistedBytes;
  const changed = await cache.write("host-a/account-a/self", next, { generation: 5 });
  assert.equal(changed.recordWrites, 1);
  assert.equal(changed.recordDeletes, 0);
  assert.equal(storage.persistedBytes - beforeBytes, new TextEncoder().encode(JSON.stringify(next[41])).byteLength);
  assert.ok(storage.persistedBytes - beforeBytes < baselineBytes / 40);

  const manifestWrites = storage.manifestWrites;
  const unchanged = await cache.write("host-a/account-a/self", next, { generation: 5 });
  assert.deepEqual(unchanged, { recordWrites: 0, recordDeletes: 0, manifestWrites: 0, refused: null });
  assert.equal(storage.manifestWrites, manifestWrites);
});

test("record scopes remain isolated and write failures do not publish partial generations", async () => {
  const storage = new FakeStorage();
  const cache = new FrontendCacheRepository(storage);
  await cache.write("host-a/account-a/peer-a", [{ kind: "row", id: "same", value: "A" }], { generation: 1 });
  await cache.write("host-a/account-a/peer-b", [{ kind: "row", id: "same", value: "B" }], { generation: 1 });
  assert.equal((await cache.read("host-a/account-a/peer-a"))?.records[0].value, "A");
  assert.equal((await cache.read("host-a/account-a/peer-b"))?.records[0].value, "B");
  assert.equal(await cache.read("host-b/account-a/peer-a"), null);

  storage.failWrites = true;
  await assert.rejects(cache.write("host-a/account-a/peer-a", [{ kind: "row", id: "same", value: "new" }], { generation: 2 }));
  storage.failWrites = false;
  assert.equal((await cache.read("host-a/account-a/peer-a"))?.records[0].value, "A");
});

test("a late generation cannot replace a newer complete transaction", async () => {
  const storage = new FakeStorage();
  const oldWriter = new FrontendCacheRepository(storage);
  const newWriter = new FrontendCacheRepository(storage);
  const hold = storage.holdNextCommit();
  const oldWrite = oldWriter.write("same-scope", [{ kind: "row", id: "current", value: "old" }], { generation: 2 });
  await hold.entered;
  const newWrite = await newWriter.write("same-scope", [{ kind: "row", id: "current", value: "new" }], { generation: 3 });
  assert.equal(newWrite.recordWrites, 1);
  hold.release();
  const late = await oldWrite;
  assert.equal(late.refused, "stale-generation");
  const complete = await oldWriter.read("same-scope");
  assert.equal(complete?.manifest.metadata.generation, 3);
  assert.equal(complete?.records[0].value, "new");
});

test("a stale invalidation cannot clear a newer complete cache revision", async () => {
  const storage = new FakeStorage();
  const cache = new FrontendCacheRepository(storage);
  const scope = "state:account-a";
  const first = [{ kind: "tab", id: "tab-a", value: { title: "first" } }];
  await cache.write(scope, first, { generation: 1 });
  await storage.commit(scope, 1, {
    scope,
    revision: 2,
    recordCount: 1,
    metadata: { generation: 2 },
  }, { upserts: [{ kind: "tab", id: "tab-a", value: { title: "newer" } }], deletes: [] });

  assert.equal(await cache.clear(scope, 1), false);
  const current = await cache.read(scope);
  assert.equal((current?.records[0].value as { title: string }).title, "newer");
});

test("duplicate record keys are refused before a transaction", async () => {
  const storage = new FakeStorage();
  const cache = new FrontendCacheRepository(storage);
  await assert.rejects(cache.write("scope", [
    { kind: "row", id: "same", value: 1 },
    { kind: "row", id: "same", value: 2 },
  ], {}));
  assert.equal(storage.manifestWrites, 0);
});
