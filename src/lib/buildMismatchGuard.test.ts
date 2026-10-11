import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { frontendApiCompatible, frontendApiContract } from "./frontendApiContract";
import { configureDaemonTransport } from "./daemonBootstrap";
import {
  checkBuildMismatch, onMismatch, getMismatch, __resetForTest,
  __setFrontendHashForTest, __setFrontendVersionForTest,
} from "./buildMismatchGuard";

beforeEach(() => { __resetForTest(); __setFrontendHashForTest("frontend-hash"); __setFrontendVersionForTest("frontend-version"); });
afterEach(() => vi.unstubAllGlobals());
function health(apiVersion: unknown, build = "independent-daemon-hash", version = "independent-daemon-version") {
  const fetcher = vi.fn(async (..._args: Parameters<typeof fetch>) => new Response(JSON.stringify({ apiVersion, build, version })));
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
