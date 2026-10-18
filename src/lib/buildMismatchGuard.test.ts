import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { frontendApiCompatible, frontendApiContract } from "./frontendApiContract";
import { configureDaemonTransport } from "./daemonBootstrap";
import {
  checkBuildMismatch, onMismatch, getMismatch, __resetForTest, installBuildDriftTriggers, recheckBuildDrift,
  resumeDeferredBuildDrift, __setBuildDriftEnvForTest, __setFrontendHashForTest, __setFrontendVersionForTest,
  type BuildDriftEnv,
} from "./buildMismatchGuard";
import { BUILD_DRIFT_RECORD_KEY, WEB_BOOT_CHANGED_EVENT, WEB_EVENTS_READY_EVENT } from "./buildDriftReload";
import { FRONTEND_BOOT_KEY } from "./daemonBootstrap";

class MemoryStorage {
  values = new Map<string, string>();
  getItem(key: string) { return this.values.get(key) ?? null; }
  setItem(key: string, value: string) { this.values.set(key, value); }
  removeItem(key: string) { this.values.delete(key); }
}
function driftEnv(patch: Partial<BuildDriftEnv> = {}) {
  const storage = new MemoryStorage();
  const reload = vi.fn();
  let visible = true;
  const env: BuildDriftEnv = {
    isWeb: () => true, storage: () => storage, origin: () => "http://127.0.0.1:7686",
    visible: () => visible, now: () => 7, reload, ...patch,
  };
  __setBuildDriftEnvForTest(env);
  return { storage, reload, setVisible: (next: boolean) => { visible = next; } };
}

beforeEach(() => {
  __resetForTest(); __setFrontendHashForTest("frontend-hash"); __setFrontendVersionForTest("frontend-version");
  __setBuildDriftEnvForTest({ isWeb: () => false, storage: () => new MemoryStorage(), origin: () => "", visible: () => true, now: () => 0, reload: () => { throw new Error("desktop reloaded"); } });
});
afterEach(() => vi.unstubAllGlobals());
function health(apiVersion: unknown, build = "independent-daemon-hash", version = "independent-daemon-version") {
  const fetcher = vi.fn(async (..._args: Parameters<typeof fetch>) => new Response(JSON.stringify({ apiVersion, build, version }), { headers: { "Content-Type": "application/json" } }));
  vi.stubGlobal("fetch", fetcher);
  return fetcher;
}

describe("frontend API compatibility", () => {
  it("accepts inclusive bounds and refuses invalid or unsupported API contracts", () => {
    const range = { version: "independent", minApiVersion: 2, maxApiVersion: 4 };
    for (const version of [2, 3, 4]) expect(frontendApiCompatible(version, range)).toBe(true);
    for (const version of [undefined, null, "2", 1, 5, 2.5, NaN]) expect(frontendApiCompatible(version, range)).toBe(false);
    expect(frontendApiCompatible(2, { ...range, minApiVersion: 4 })).toBe(false);
    expect(frontendApiCompatible(2, { ...range, maxApiVersion: 1 })).toBe(false);
    expect(frontendApiCompatible(0, { ...range, minApiVersion: 0 })).toBe(false);
  });
  it("accepts independently released frontend and daemon versions and build identities", async () => {
    health(frontendApiContract.minApiVersion);
    expect(await checkBuildMismatch("http://localhost:7686")).toBeNull();
  });
  it.each([1, frontendApiContract.maxApiVersion + 1, undefined])("reports unsupported API %s regardless of matching build identities", async apiVersion => {
    health(apiVersion, "frontend-hash", "frontend-version");
    const result = await checkBuildMismatch("http://localhost:7686");
    expect(result).toMatchObject({ reason: "api", apiVersion: apiVersion ?? null, minApiVersion: frontendApiContract.minApiVersion, maxApiVersion: frontendApiContract.maxApiVersion });
    expect(getMismatch()).toBe(result);
  });
  it("notifies only active subscribers and caches a proven identity", async () => {
    const fetcher = health(1);
    const active = vi.fn(); const removed = vi.fn();
    onMismatch(active); onMismatch(removed)();
    const result = await checkBuildMismatch("http://localhost:7686");
    expect(await checkBuildMismatch("http://localhost:7686")).toBe(result);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(active).toHaveBeenCalledWith(result);
    expect(removed).not.toHaveBeenCalled();
  });
  it("does not classify a network failure as incompatibility or cache it as proof", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("unreachable")));
    expect(await checkBuildMismatch("http://localhost:7686")).toBeNull();
    const fetcher = health(1);
    expect(await checkBuildMismatch("http://localhost:7686")).not.toBeNull();
    expect(fetcher).toHaveBeenCalledTimes(1);
  });
  it("checks the configured base daemon while the frontend uses another origin", async () => {
    const fetcher = health(frontendApiContract.minApiVersion);
    configureDaemonTransport({ origin: window.location.origin, token: "fixture-token" });
    expect(await checkBuildMismatch()).toBeNull();
    expect(fetcher.mock.calls[0]?.[0]).toBe(`${window.location.origin}/api/health`);
    const init = fetcher.mock.calls[0]?.[1] as RequestInit;
    expect(new Headers(init.headers).get("Authorization")).toBe("Bearer fixture-token");
  });
});

describe("web build drift reload", () => {
  const compatible = () => frontendApiContract.minApiVersion;
  it("reloads once per daemon build, then shows the build banner", async () => {
    const { storage, reload } = driftEnv();
    health(compatible(), "daemon-build");
    expect(await checkBuildMismatch("http://localhost:7686")).toBeNull();
    expect(reload).toHaveBeenCalledTimes(1);
    expect(JSON.parse(storage.getItem(BUILD_DRIFT_RECORD_KEY)!)).toEqual({ origin: "http://127.0.0.1:7686", build: "daemon-build", at: 7 });
    __resetForTest();
    __setFrontendHashForTest("frontend-hash");
    driftEnv({ storage: () => storage, reload });
    health(compatible(), "daemon-build");
    expect(await checkBuildMismatch("http://localhost:7686")).toMatchObject({ reason: "build", backendHash: "daemon-build" });
    expect(reload).toHaveBeenCalledTimes(1);
  });
  it("shows the banner instead of reloading when the record does not read back", async () => {
    const reload = vi.fn();
    driftEnv({ reload, storage: () => ({ getItem: () => null, setItem: () => {}, removeItem: () => {} }) });
    health(compatible(), "daemon-build");
    expect(await checkBuildMismatch("http://localhost:7686")).toMatchObject({ reason: "build" });
    expect(reload).not.toHaveBeenCalled();
  });
  it("leaves a signed release page to the frontend update runtime but still checks its API", async () => {
    const { storage, reload } = driftEnv();
    storage.setItem(FRONTEND_BOOT_KEY, JSON.stringify({ ticket: { releaseId: "frontend-v1.0.0", generation: 1 } }));
    expect(new URL("/frontend/frontend-v1.0.0/index.html", "http://127.0.0.1:7686").pathname.startsWith("/frontend/")).toBe(true);
    health(compatible(), "daemon-build");
    expect(await checkBuildMismatch("http://localhost:7686")).toBeNull();
    expect(reload).not.toHaveBeenCalled();
    expect(storage.getItem(BUILD_DRIFT_RECORD_KEY)).toBeNull();
    __resetForTest();
    driftEnv({ storage: () => storage, reload });
    health(1, "daemon-build");
    expect(await checkBuildMismatch("http://localhost:7686")).toMatchObject({ reason: "api" });
  });
  it("ignores a mesh peer's boot-changed build and checks the local daemon on reconnect", async () => {
    const { storage, reload } = driftEnv();
    configureDaemonTransport({ origin: window.location.origin, token: "fixture-token" });
    const fetcher = health(compatible(), "frontend-hash");
    const uninstall = installBuildDriftTriggers();
    try {
      window.dispatchEvent(new CustomEvent(WEB_BOOT_CHANGED_EVENT, { detail: { boot: "peer-boot", build: "peer-build", relayed: true } }));
      await Promise.resolve();
      expect(fetcher).not.toHaveBeenCalled();
      expect(reload).not.toHaveBeenCalled();
      expect(storage.getItem(BUILD_DRIFT_RECORD_KEY)).toBeNull();
      const local = health(compatible(), "local-build");
      window.dispatchEvent(new Event(WEB_EVENTS_READY_EVENT));
      await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
      expect(local.mock.calls[0]?.[0]).toBe(`${window.location.origin}/api/health`);
    } finally { uninstall(); }
  });
  it("keeps a same-build boot attach in place and reloads on a changed local build hint", async () => {
    const { reload } = driftEnv();
    const fetcher = health(compatible(), "unused");
    const uninstall = installBuildDriftTriggers();
    try {
      window.dispatchEvent(new CustomEvent(WEB_BOOT_CHANGED_EVENT, { detail: { boot: "b", build: "frontend-hash", relayed: false } }));
      expect(reload).not.toHaveBeenCalled();
      window.dispatchEvent(new CustomEvent(WEB_BOOT_CHANGED_EVENT, { detail: { boot: "b", build: "new-build", relayed: false } }));
      await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
      expect(fetcher).not.toHaveBeenCalled();
    } finally { uninstall(); }
  });
  it("drops a stale health response when a newer trigger already decided", async () => {
    const { reload } = driftEnv();
    let resolveStale!: (r: Response) => void;
    vi.stubGlobal("fetch", vi.fn(() => new Promise<Response>(resolve => { resolveStale = resolve; })));
    const stale = checkBuildMismatch("http://localhost:7686");
    await recheckBuildDrift("frontend-hash");
    resolveStale(new Response(JSON.stringify({ apiVersion: compatible(), build: "stale-build" })));
    await stale;
    expect(reload).not.toHaveBeenCalled();
  });
  it("defers a hidden page until it becomes visible, without a second fetch", async () => {
    const { reload, setVisible } = driftEnv();
    setVisible(false);
    const fetcher = health(compatible(), "daemon-build");
    await checkBuildMismatch("http://localhost:7686");
    expect(reload).not.toHaveBeenCalled();
    setVisible(true);
    resumeDeferredBuildDrift();
    expect(reload).toHaveBeenCalledTimes(1);
    expect(fetcher).toHaveBeenCalledTimes(1);
  });
  it("keeps the banner, not a reload, when health is unreadable after an attempted reload", async () => {
    const { storage, reload } = driftEnv();
    storage.setItem(BUILD_DRIFT_RECORD_KEY, JSON.stringify({ origin: "http://127.0.0.1:7686", build: "daemon-build", at: 1 }));
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
    expect(await checkBuildMismatch("http://localhost:7686")).toMatchObject({ reason: "build-unreachable" });
    expect(reload).not.toHaveBeenCalled();
    expect(storage.getItem(BUILD_DRIFT_RECORD_KEY)).not.toBeNull();
  });
});
