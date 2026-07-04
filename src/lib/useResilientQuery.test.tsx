// Run: npx tsx src/lib/useResilientQuery.test.tsx

import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { UseResilientQueryResult } from "./useResilientQuery";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function currentReconnect(state: UseResilientQueryResult<string[]> | null): boolean {
  return state?.reconnecting === true;
}

let fetchAttempts = 0;
let shouldFail = true;

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/resilient-query" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function renderProbe(
  onUpdate: (state: UseResilientQueryResult<string[]>) => void,
): Promise<{ root: Root }> {
  const { useResilientQuery } = await import("./useResilientQuery");
  function Probe() {
    const state = useResilientQuery(
      async () => {
        fetchAttempts++;
        if (shouldFail) throw new TypeError("Load failed");
        return ["plan-a"];
      },
      [],
      { deps: [] },
    );
    useEffect(() => {
      onUpdate(state);
    }, [state]);
    return null;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<Probe />);
  });
  return { root };
}

test("failing fetch surfaces friendly error then recovers", async () => {
  fetchAttempts = 0;
  shouldFail = true;
  const holder: { current: UseResilientQueryResult<string[]> | null } = { current: null };
  const { root } = await renderProbe((state) => {
    holder.current = state;
  });

  await act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  assert(holder.current, "state captured");
  assert(holder.current.error?.includes("Reconnecting"), "friendly error, not raw TypeError");
  assert(!holder.current.error?.includes("Load failed"), "raw message hidden");
  assert(holder.current.data.length === 0, "no data yet");

  shouldFail = false;
  await act(async () => {
    await holder.current!.refetch();
  });
  const recovered = holder.current!;
  assert(recovered.data.length === 1, "recovered data");
  assert(recovered.error === null, "error cleared after recovery");
  assert(fetchAttempts >= 2, "retried fetch");
  await act(async () => root.unmount());
});

test("stale data stays visible while reconnecting", async () => {
  fetchAttempts = 0;
  shouldFail = false;
  const holder: { current: UseResilientQueryResult<string[]> | null } = { current: null };
  const { root } = await renderProbe((state) => {
    holder.current = state;
  });

  await act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  assert(holder.current?.data.length === 1, "initial load");

  shouldFail = true;
  await act(async () => {
    await holder.current!.refetch();
  });
  assert(holder.current?.data.length === 1, "stale data kept during failure");
  assert(!holder.current?.error, "no blocking error with stale data");
  await act(async () => root.unmount());
});

test("stale refetch is silent until two consecutive failures and clears on success", async () => {
  fetchAttempts = 0;
  shouldFail = false;
  const holder: { current: UseResilientQueryResult<string[]> | null } = { current: null };
  const { root } = await renderProbe((state) => {
    holder.current = state;
  });

  await act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  assert(holder.current?.data.length === 1, "initial data loaded");
  assert(!currentReconnect(holder.current), "success is not reconnecting");

  await act(async () => {
    await holder.current!.refetch();
  });
  assert(!currentReconnect(holder.current), "healthy stale refetch stays quiet");

  shouldFail = true;
  await act(async () => {
    await holder.current!.refetch();
  });
  assert(!currentReconnect(holder.current), "first stale failure stays quiet");

  await act(async () => {
    await holder.current!.refetch();
  });
  assert(currentReconnect(holder.current), "second stale failure shows reconnecting");

  shouldFail = false;
  await act(async () => {
    await holder.current!.refetch();
  });
  assert(!currentReconnect(holder.current), "first success clears reconnecting");

  await act(async () => root.unmount());
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
