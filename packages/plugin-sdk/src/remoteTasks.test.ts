import assert from "node:assert/strict";
import {test} from "node:test";
import {RemoteTaskController, type RemoteTaskBackend, type RemoteTaskBinding, type RemoteTaskStorage} from "./remoteTasks";
import {RemoteTaskFileStorage} from "./remoteTaskStorage";

function fixture() {
  const persisted = new Map<string, RemoteTaskBinding>();
  const storage: RemoteTaskStorage = {
    read(id) { return structuredClone(persisted.get(id) ?? null); },
    compareExchange(id, revision, next) {
      if ((persisted.get(id)?.revision ?? null) !== revision) return false;
      persisted.set(id, structuredClone(next));
      return true;
    },
  };
  let creates = 0;
  let reads = 0;
  let uncertain = false;
  const environment = {id: "environment-fixture", accountRef: "account-fixture", label: "Fixture environment"};
  const backend: RemoteTaskBackend = {
    capability: {backend: "fixture", mode: "cloud", displayName: "Cloud", environments: [environment], availability: {status: "available"}, followUp: false, cancel: false, applyDiff: false},
    environments() { return [environment]; },
    create() { creates++; if (uncertain) throw new Error("connection lost after server accepted"); return {taskId: "task-fixture"}; },
    read(binding) {
      reads++;
      return {taskId: binding.snapshot!.taskId, attempt: "attempt-1", status: "ready", result: "README title", diff: null, cursor: "cursor-fixture", error: null};
    },
  };
  return {storage, backend, persisted, environment, creates: () => creates, reads: () => reads, failCreate: () => {uncertain = true;}};
}

test("restart re-fetches the persisted exact remote task without another create", async () => {
  const f = fixture();
  await new RemoteTaskController(f.storage, f.backend).create("tab", "provider-fixture", f.environment, "master", "Read README title");
  const restored = await new RemoteTaskController(f.storage, f.backend).restore("tab");
  assert.equal(f.creates(), 1);
  assert.equal(f.reads(), 1);
  assert.equal(restored?.snapshot?.result, "README title");
  assert.equal(restored?.snapshot?.cursor, "cursor-fixture");
  assert.equal(restored?.accountRef, f.environment.accountRef);
  assert.equal(restored?.snapshot?.diff, null);
});

test("a lost create acknowledgement survives restart and refuses replay", async () => {
  const f = fixture(); f.failCreate();
  await new RemoteTaskController(f.storage, f.backend).create("tab", "provider-fixture", f.environment, "master", "Read README title");
  const restarted = new RemoteTaskController(f.storage, f.backend);
  assert.equal((await restarted.restore("tab"))?.phase, "create_uncertain");
  await assert.rejects(async () => restarted.create("tab", "provider-fixture", f.environment, "master", "Read README title"));
  assert.equal(f.creates(), 1);
  assert.equal(f.reads(), 0);
});

test("concurrent submissions claim durable ownership before the backend call", async () => {
  const f = fixture();
  const results = await Promise.allSettled([1, 2].map(async () => new RemoteTaskController(f.storage, f.backend).create("tab", "provider-fixture", f.environment, "master", "Read README title")));
  assert.equal(results.filter(result => result.status === "fulfilled").length, 1);
  assert.equal(f.creates(), 1);
});

test("unavailable capability and foreign environments refuse before creating", async () => {
  const f = fixture();
  const controller = new RemoteTaskController(f.storage, f.backend);
  await assert.rejects(async () => controller.create("tab", "provider-fixture", {...f.environment, accountRef: "foreign"}, "master", "Read README"));
  f.backend.capability.availability = {status: "unavailable", reason: "Connect an account with Cloud environments"};
  await assert.rejects(async () => controller.create("tab", "provider-fixture", f.environment, "master", "Read README"));
  assert.equal(f.creates(), 0);
});

test("crash during create claim becomes uncertain and a foreign task response is rejected", async () => {
  const f = fixture();
  const controller = new RemoteTaskController(f.storage, f.backend);
  await controller.create("tab", "provider-fixture", f.environment, "master", "Read README");
  const saved = f.persisted.get("tab")!;
  f.persisted.set("crashed", {...saved, phase: "creating", snapshot: null});
  assert.equal((await controller.restore("crashed"))?.phase, "create_uncertain");
  f.backend.read = () => ({...saved.snapshot!, taskId: "foreign"});
  await assert.rejects(async () => controller.restore("tab"));
  assert.equal(f.persisted.get("tab")?.snapshot?.taskId, "task-fixture");
});

test("provider file storage preserves binding across controller instances and releases failed writes", async () => {
  const f = fixture();
  const files = new Map<string, string>();
  const locks = new Set<string>();
  let writable = true;
  const fs = {
    readFile: (path: string) => files.get(path) ?? null,
    writeFileAtomic(path: string, value: string) { if (!writable) return false; files.set(path, value); return true; },
    lockFile(path: string) { if (locks.has(path)) return null; locks.add(path); return path; },
    unlockFile(path: string) { return locks.delete(path); },
    makeDirs: () => true,
  };
  await new RemoteTaskController(new RemoteTaskFileStorage(fs, "/provider-data"), f.backend)
    .create("tab", "provider-fixture", f.environment, "master", "Read README");
  const restarted = new RemoteTaskController(new RemoteTaskFileStorage(fs, "/provider-data"), f.backend);
  assert.equal((await restarted.restore("tab"))?.snapshot?.status, "ready");
  assert.equal(f.creates(), 1);
  writable = false;
  await assert.rejects(async () => restarted.restore("tab"));
  assert.equal(locks.size, 0);
  await assert.rejects(async () => new RemoteTaskFileStorage(fs, "/provider-data").read("../foreign"));
});

test("an acknowledged create whose binding write fails is never recreated after restart", () => {
  const f = fixture();
  let failAcknowledgement = true;
  const storage: RemoteTaskStorage = {...f.storage, compareExchange(id, revision, next) {
    if (failAcknowledgement && next.phase === "bound") return false;
    return f.storage.compareExchange(id, revision, next);
  }};
  assert.throws(() => new RemoteTaskController(storage, f.backend).create("tab", "provider-fixture", f.environment, "master", "Read README"));
  failAcknowledgement = false;
  const restarted = new RemoteTaskController(storage, f.backend);
  assert.equal(restarted.restore("tab")?.phase, "create_uncertain");
  assert.throws(() => restarted.create("tab", "provider-fixture", f.environment, "master", "Read README"));
  assert.equal(f.creates(), 1);
});

test("restore refuses a binding from another execution mode without creating or reading a task", () => {
  const f = fixture();
  new RemoteTaskController(f.storage, f.backend).create("tab", "provider-fixture", f.environment, "master", "Read README");
  const foreign = {...f.storage.read("tab")!, executionMode: "foreign"};
  const storage = {...f.storage, read: () => foreign};
  assert.throws(() => new RemoteTaskController(storage, f.backend).restore("tab"));
  assert.equal(f.creates(), 1);
});
