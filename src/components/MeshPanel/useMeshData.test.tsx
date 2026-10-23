import assert from "node:assert/strict";
import { act } from "react";
import { it } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { configureDaemonTransport } from "../../lib/daemonBootstrap";
import { setFrontendCacheStorageForTests, type FrontendCacheChanges, type FrontendCacheCommit, type FrontendCacheManifest, type FrontendCacheRecord, type FrontendCacheSnapshot, type FrontendCacheStorage } from "../../lib/frontendCacheRecords";
import { installDom, mount, settle } from "../Jobs/jobsEntry.testdata";
import { daemon } from "../General/generalOverview.testdata";
import { useMeshData, type MeshDataResult } from "./useMeshData";

class FakeStorage implements FrontendCacheStorage {
  private readonly scopes = new Map<string, { manifest: FrontendCacheManifest; records: Map<string, FrontendCacheRecord> }>();
  private readGate?: { entered: () => void; done: () => void; wait: Promise<void> };
  commits: Array<{ writes: number; bytes: number }> = [];
  private commitWaiters: Array<() => void> = [];

  waitForNextCommit(): Promise<void> {
    return new Promise<void>((resolve) => { this.commitWaiters.push(resolve); });
  }

  holdNextRead(): { entered: Promise<void>; finished: Promise<void>; release: () => void } {
    let entered!: () => void;
    let release!: () => void;
    let done!: () => void;
    const enteredPromise = new Promise<void>((resolve) => { entered = resolve; });
    const finished = new Promise<void>((resolve) => { done = resolve; });
    const wait = new Promise<void>((resolve) => { release = resolve; });
    this.readGate = { entered, done, wait };
    return { entered: enteredPromise, finished, release };
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
    for (const row of changes.upserts) records.set(`${row.kind}\u0000${row.id}`, structuredClone(row));
    assert.equal(records.size, manifest.recordCount);
    this.scopes.set(scope, { manifest: structuredClone(manifest), records });
    this.commits.push({
      writes: changes.upserts.length,
      bytes: new TextEncoder().encode(JSON.stringify(changes.upserts)).byteLength,
    });
    this.commitWaiters.shift()?.();
    return "committed";
  }

  async clear(scope: string, expectedRevision?: number): Promise<boolean> {
    const current = this.scopes.get(scope);
    if (expectedRevision !== undefined && (current?.manifest.revision ?? 0) !== expectedRevision) return false;
    return this.scopes.delete(scope);
  }
}

it("refreshes Mesh inventory from events and keeps known rows across failures", async () => {
  installDom();
  configureDaemonTransport({ origin: "http://localhost", token: "mesh-cache-fixture" });
  const cache = new FakeStorage();
  setFrontendCacheStorageForTests(cache);
  localStorage.clear();
  let fail = false;
  let service = daemon;
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "mesh_inventory_list") {
      if (fail) throw new Error("inventory transport unavailable");
      return { resources: [service] } as T;
    }
    if (command === "mesh_get_status") return { instance_name: "Local" } as T;
    if (command === "mesh_list_peers") return { paired: [], discovered: [] } as T;
    throw new Error(command);
  });
  let state: MeshDataResult | undefined;
  function Probe() {
    state = useMeshData(0);
    return null;
  }
  const initialCommit = cache.waitForNextCommit();
  const view = await mount(<Probe />);
  let viewClosed = false;
  try {
    await settle(() => state?.loading === false);
    await initialCommit;
    assert.deepEqual(state?.services.map((service) => service.id), [daemon.id]);
    assert.equal(cache.commits.length, 1);
    assert.equal(cache.commits[0].writes, 2);
    await act(async () => { await state?.refresh(); });
    assert.equal(cache.commits.length, 1, "identical refresh performs no cache transaction");
    fail = true;
    await act(async () => { await state?.refresh(); });
    assert.deepEqual(state?.services.map((service) => service.id), [daemon.id]);
    assert.equal(state?.loading, false);
    assert.ok(state?.error);
    fail = false;
    service = { ...daemon, name: "Changed" };
    const changedCommit = cache.waitForNextCommit();
    await act(async () => { await state?.refresh(); });
    await changedCommit;
    assert.equal(state?.error, null);
    assert.equal(cache.commits.length, 2);
    assert.equal(cache.commits[1].writes, 1, "one changed service writes its record only");
    console.log(`synthetic Mesh cache: ${cache.commits[0].writes} records/${cache.commits[0].bytes} bytes -> ${cache.commits[1].writes} record/${cache.commits[1].bytes} bytes`);
    console.log("inventory failure preserves known rows and successful refresh clears failure");
    await view.close();
    viewClosed = true;

    const readGate = cache.holdNextRead();
    let lateState: MeshDataResult | undefined;
    function LateProbe() { lateState = useMeshData(0); return null; }
    const lateView = await mount(<LateProbe />);
    await readGate.entered;
    await settle(() => lateState?.loading === false);
    assert.equal(lateState?.services[0].name, "Changed", "live inventory is visible before delayed hydration");
    await act(async () => { readGate.release(); await readGate.finished; });
    assert.equal(lateState?.services[0].name, "Changed", "late cache hydration cannot replace accepted live inventory");
    await lateView.close();
  } finally {
    if (!viewClosed) await view.close();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});

it("surfaces a timed-out peer load as an error and releases the refresh for the next cycle", async () => {
  installDom();
  configureDaemonTransport({ origin: "http://localhost", token: "mesh-timeout-fixture" });
  setFrontendCacheStorageForTests(new FakeStorage());
  localStorage.clear();
  let peersTimeOut = true;
  const peer = {
    instance_id: "peer-1", instance_name: "Peer", addresses: [], api_port: 7685, online: true,
    access_level: "full", last_seen: null, relay_protocol_version: 2,
  };
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "mesh_inventory_list") return { resources: [] } as T;
    if (command === "mesh_get_status") return { instance_name: "Local" } as T;
    if (command === "mesh_list_peers") {
      if (peersTimeOut) throw new DOMException("signal timed out", "TimeoutError");
      return { paired: [peer] } as T;
    }
    if (command === "mesh_peer_services_list") {
      return { services: [], stale: false, fetched_at: null, peer_online: true, error: null } as T;
    }
    throw new Error(command);
  });
  let state: MeshDataResult | undefined;
  function Probe() {
    state = useMeshData(0);
    return null;
  }
  const view = await mount(<Probe />);
  try {
    await settle(() => state?.loading === false && state.error !== null);
    assert.match(String(state?.error), /timed out/);
    assert.equal(state?.peers.length, 0);
    peersTimeOut = false;
    await act(async () => { await state?.refresh(); });
    assert.equal(state?.error, null);
    assert.deepEqual(state?.peers.map((candidate) => candidate.instance_id), ["peer-1"]);
  } finally {
    await view.close();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});
