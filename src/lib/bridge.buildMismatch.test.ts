// Run: npx tsx src/lib/bridge.buildMismatch.test.ts

import { invoke, __setInvokeForTest } from "./bridge";
import {
  getMismatch,
  onMismatch,
  __resetForTest,
  __setFrontendHashForTest,
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

function mockFetch(build: string, status = 200) {
  (globalThis as unknown as Record<string, unknown>).fetch = async () => ({
    ok: status < 400,
    status,
    json: async () => ({ status: "ok", build }),
  });
}

async function main() {
  await test("command-not-found error with mismatch → mismatch is detected (not raw throw only)", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch("112233");

    let mismatchFired = false;
    const unlisten = onMismatch((m) => { mismatchFired = !!m; });

    __setInvokeForTest(async () => {
      throw new Error("Command mux_spawn_tab not found");
    });

    let threw = false;
    try {
      await invoke("mux_spawn_tab", {});
    } catch {
      threw = true;
    }

    await new Promise((r) => setTimeout(r, 0));

    assert(threw, "invoke should still throw");
    assert(mismatchFired, "mismatch subscriber should have fired");
    assert(getMismatch() !== null, "mismatch should be recorded");

    unlisten();
    __setInvokeForTest(null);
  });

  await test("command-not-found with matching hashes → error thrown, no mismatch", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch("aabbcc");

    let mismatchFired = false;
    const unlisten = onMismatch(() => { mismatchFired = true; });

    __setInvokeForTest(async () => {
      throw new Error("Command mux_spawn_tab not found");
    });

    let threw = false;
    try {
      await invoke("mux_spawn_tab", {});
    } catch {
      threw = true;
    }

    await new Promise((r) => setTimeout(r, 0));

    assert(threw, "invoke should still throw");
    assert(!mismatchFired, "no mismatch when hashes match");

    unlisten();
    __setInvokeForTest(null);
  });

  await test("non-command-not-found error → does NOT trigger mismatch check", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    let fetchCalled = false;
    (globalThis as unknown as Record<string, unknown>).fetch = async () => {
      fetchCalled = true;
      return { ok: true, status: 200, json: async () => ({ status: "ok", build: "112233" }) };
    };

    __setInvokeForTest(async () => {
      throw new Error("Some other unrelated error");
    });

    try {
      await invoke("mux_get_state", {});
    } catch {
    }

    await new Promise((r) => setTimeout(r, 0));

    assert(!fetchCalled, "health should not be fetched for non-command-not-found errors");

    __setInvokeForTest(null);
  });

  await test("successful invoke → no mismatch check triggered", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    let fetchCalled = false;
    (globalThis as unknown as Record<string, unknown>).fetch = async () => {
      fetchCalled = true;
      return { ok: true, status: 200, json: async () => ({ status: "ok", build: "112233" }) };
    };

    __setInvokeForTest(async <T,>() => ({ tabs: {}, workspaces: [] } as T));

    await invoke("mux_get_state", {});
    await new Promise((r) => setTimeout(r, 0));

    assert(!fetchCalled, "health should not be fetched on successful invoke");

    __setInvokeForTest(null);
  });

  console.log(`bridge.buildMismatch tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
