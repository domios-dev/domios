
class FakeStorage {
  private map = new Map<string, string>();
  getItem(key: string): string | null {
    return this.map.has(key) ? this.map.get(key)! : null;
  }
  setItem(key: string, value: string): void {
    this.map.set(key, value);
  }
  removeItem(key: string): void {
    this.map.delete(key);
  }
  clear(): void {
    this.map.clear();
  }
  keys(): string[] {
    return [...this.map.keys()];
  }
}

const fakeLocalStorage = new FakeStorage();
(globalThis as unknown as { window: unknown }).window = {
  location: { protocol: "http:", host: "localhost:7686" },
};
(globalThis as unknown as { localStorage: unknown }).localStorage = fakeLocalStorage;
(globalThis as unknown as { sessionStorage: unknown }).sessionStorage = new FakeStorage();
// No indexedDB global — exercises the "storage unavailable" degrade path too.

let passed = 0;
let failed = 0;
function assert(label: string, cond: unknown) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

async function main() {
  const { loadCachedStateSync, saveStateSnapshot, clearStateSnapshot, createWriteGate } =
    await import("./stateCache");
  const { IS_WEB } = await import("./webApi");

  assert("IS_WEB resolves true under the faked window", IS_WEB === true);

  function sampleState(active: string) {
    return {
      workspaces: [{ id: active, name: "W", tabs: [], active_tab_id: null, cwd: null }],
      tabs: {},
      active_workspace_id: active,
      focused_tab_id: null,
      generation: 7,
    } as Parameters<typeof saveStateSnapshot>[0];
  }

  console.log("stateCache: save/load round-trip:");
  fakeLocalStorage.clear();
  (globalThis as unknown as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "build-1";
  saveStateSnapshot(sampleState("ws-a"));
  const loaded = loadCachedStateSync();
  assert("round-trips the saved state", loaded?.active_workspace_id === "ws-a");

  console.log("\nstateCache: version-mismatch drop:");
  (globalThis as unknown as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "build-2";
  const afterBuildBump = loadCachedStateSync();
  assert("build-hash mismatch drops the cache instead of migrating", afterBuildBump === null);
  assert("mismatched entry is cleared, not left behind", loadCachedStateSync() === null);

  fakeLocalStorage.clear();
  (globalThis as unknown as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "build-1";
  saveStateSnapshot(sampleState("ws-b"));
  const key = fakeLocalStorage.keys()[0];
  const raw = JSON.parse(fakeLocalStorage.getItem(key)!);
  raw.schemaVersion = 999;
  fakeLocalStorage.setItem(key, JSON.stringify(raw));
  assert("schema-version mismatch drops the cache", loadCachedStateSync() === null);

  console.log("\nstateCache: clearStateSnapshot:");
  saveStateSnapshot(sampleState("ws-c"));
  assert("state is cached before clear", loadCachedStateSync() !== null);
  clearStateSnapshot();
  assert("state is gone after clear", loadCachedStateSync() === null);

  console.log("\nstateCache: write gate (stale-write gate):");
  const gate = createWriteGate(true);
  assert("starts locked when constructed locked", gate.canWrite() === false);
  gate.unlock();
  assert("unlock() opens the gate", gate.canWrite() === true);
  gate.lock();
  assert("lock() closes the gate again", gate.canWrite() === false);

  const defaultGate = createWriteGate();
  assert("defaults to unlocked", defaultGate.canWrite() === true);

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
