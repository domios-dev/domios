// Run: npx tsx src/lib/buildMismatchGuard.test.ts

import {
  checkBuildMismatch,
  onMismatch,
  getMismatch,
  __resetForTest,
  __setFrontendHashForTest,
  __setFrontendVersionForTest,
} from "./buildMismatchGuard";

let failed = 0;
async function test(name: string, fn: () => void | Promise<void>) {
  try {
    await fn();
    console.log(`✓ ${name}`);
  } catch (e) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(e);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function mockFetch(build: string | null, status = 200, version?: string) {
  (globalThis as unknown as Record<string, unknown>).fetch = async (_url: unknown) => ({
    ok: status < 400,
    status,
    json: async () => ({ status: "ok", build, version }),
  });
}

async function main() {
  await test("mismatch: backend hash differs from frontend hash → returns mismatch info", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch("112233");

    const result = await checkBuildMismatch("http://localhost:7686");

    assert(result !== null, "should return mismatch");
    assert(result.frontendHash === "aabbcc", `frontendHash: ${String(result?.frontendHash)}`);
    assert(result.backendHash === "112233", `backendHash: ${String(result?.backendHash)}`);
  });

  await test("match: same hash on both sides → returns null", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch("aabbcc");

    const result = await checkBuildMismatch("http://localhost:7686");

    assert(result === null, "no mismatch expected");
  });

  await test("stale packaged window: 1.7.2 UI attached to 1.6.2 daemon is classified", async () => {
    __resetForTest();
    __setFrontendHashForTest("packaged-172");
    __setFrontendVersionForTest("1.7.2");
    mockFetch("packaged-172", 200, "1.6.2");

    const result = await checkBuildMismatch("http://localhost:7685");

    assert(result !== null, "version skew should return mismatch");
    assert(result.reason === "version", `reason: ${String(result?.reason)}`);
    assert(result.frontendVersion === "1.7.2", `frontendVersion: ${String(result?.frontendVersion)}`);
    assert(result.backendVersion === "1.6.2", `backendVersion: ${String(result?.backendVersion)}`);
  });

  await test("singleton: second call returns cached result, does not re-fetch", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    let fetchCount = 0;
    (globalThis as unknown as Record<string, unknown>).fetch = async () => {
      fetchCount++;
      return { ok: true, status: 200, json: async () => ({ status: "ok", build: "112233" }) };
    };

    await checkBuildMismatch("http://localhost:7686");
    await checkBuildMismatch("http://localhost:7686");

    assert(fetchCount === 1, `expected 1 fetch, got ${fetchCount}`);
  });

  await test("getMismatch returns current state without re-fetching", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch("112233");

    assert(getMismatch() === null, "initially null");
    await checkBuildMismatch("http://localhost:7686");
    assert(getMismatch() !== null, "mismatch after check");
    assert(getMismatch()?.backendHash === "112233", "correct backend hash");
  });

  await test("onMismatch subscriber fires when mismatch detected", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch("112233");

    let fired = false;
    const unlisten = onMismatch((m) => { fired = !!m; });

    await checkBuildMismatch("http://localhost:7686");

    assert(fired, "subscriber should have fired");
    unlisten();
  });

  await test("onMismatch subscriber does NOT fire when hashes match", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch("aabbcc");

    let fired = false;
    const unlisten = onMismatch(() => { fired = true; });

    await checkBuildMismatch("http://localhost:7686");

    assert(!fired, "subscriber should not fire on match");
    unlisten();
  });

  await test("onMismatch unlisten removes subscriber", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch("112233");

    let count = 0;
    const unlisten = onMismatch(() => { count++; });
    unlisten(); // remove before check

    await checkBuildMismatch("http://localhost:7686");

    assert(count === 0, "unlistened subscriber should not fire");
  });

  await test("health fetch failure → no mismatch (fail-open)", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    (globalThis as unknown as Record<string, unknown>).fetch = async () => {
      throw new Error("network error");
    };

    const result = await checkBuildMismatch("http://localhost:7686");

    assert(result === null, "network error should not trigger mismatch");
  });

  await test("health returns null build → no mismatch (fail-open)", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch(null);

    const result = await checkBuildMismatch("http://localhost:7686");

    assert(result === null, "missing build field should not trigger mismatch");
  });

  console.log(`buildMismatchGuard tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
