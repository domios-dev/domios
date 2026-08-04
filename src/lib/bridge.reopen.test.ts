// Run: npx tsx src/lib/bridge.reopen.test.ts

import { reopenClosedTab, __setInvokeForTest } from "./bridge";

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

async function main() {
  await test("reopenClosedTab invokes mux_reopen_closed_tab with workspaceId + index", async () => {
    const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
    __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args });
      return { id: "t1", title: "t", tab_id: "p1" } as T;
    });

    await reopenClosedTab("ws-42", 2);

    assert(calls.length === 1, `expected 1 invoke, got ${calls.length}`);
    assert(calls[0].cmd === "mux_reopen_closed_tab", `wrong command: ${calls[0].cmd}`);
    assert(calls[0].args?.workspaceId === "ws-42", "passes workspaceId");
    assert(calls[0].args?.index === 2, "passes index");
    __setInvokeForTest(null);
  });

  await test("reopenClosedTab defaults to the most recent (index 0) when omitted", async () => {
    const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
    __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args });
      return {} as T;
    });

    await reopenClosedTab("ws-42");

    assert(calls[0].args?.index === 0, `expected default index 0, got ${String(calls[0].args?.index)}`);
    __setInvokeForTest(null);
  });

  console.log(`bridge.reopen tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
