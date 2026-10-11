// Run: npx tsx src/lib/bridge.buildMismatch.test.ts

import { configureDaemonTransport } from "./daemonBootstrap";
import { invoke, __setInvokeForTest } from "./bridge";
import {
  getMismatch,
  checkBuildMismatch,
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

function mockFetch(apiVersion: number) {
  globalThis.fetch = async () => new Response(JSON.stringify({ status: "ok", build: "independent-daemon", apiVersion }), {
    headers: { "Content-Type": "application/json" },
  });
}

async function main() {
  configureDaemonTransport({ origin: "http://127.0.0.1:7686", token: "fixture-token" });
  await test("command-not-found with incompatible API reports incompatibility and preserves the error", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch(1);

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

    await checkBuildMismatch();

    assert(threw, "invoke should still throw");
    assert(mismatchFired, "mismatch subscriber should have fired");
    assert(getMismatch() !== null, "mismatch should be recorded");

    unlisten();
    __setInvokeForTest(null);
  });

  await test("command-not-found with compatible API preserves the error without a mismatch", async () => {
    __resetForTest();
    __setFrontendHashForTest("aabbcc");
    mockFetch(2);

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

    await checkBuildMismatch();

    assert(threw, "invoke should still throw");
    assert(!mismatchFired, "no mismatch for a compatible API");

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

    assert(!fetchCalled, "health should not be fetched on successful invoke");

    __setInvokeForTest(null);
  });

  console.log(`bridge.buildMismatch tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
