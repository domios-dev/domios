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
function assert(condition: unknown, message: string): void {
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

test("a success between failures resets the counter (no flap)", async () => {
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

  shouldFail = true;
  await act(async () => {
    await holder.current!.refetch();
  });
  assert(!currentReconnect(holder.current), "one failure stays quiet");

  shouldFail = false;
  await act(async () => {
    await holder.current!.refetch();
  });
  assert(!currentReconnect(holder.current), "intervening success resets counter");

  shouldFail = true;
  await act(async () => {
    await holder.current!.refetch();
  });
  assert(!currentReconnect(holder.current), "isolated failure after success stays quiet");

  await act(async () => root.unmount());
});

test("reconnecting never shows before any data exists", async () => {
  fetchAttempts = 0;
  shouldFail = true;
  const holder: { current: UseResilientQueryResult<string[]> | null } = { current: null };
  const { root } = await renderProbe((state) => {
    holder.current = state;
  });

  await act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  assert(holder.current?.data.length === 0, "no data on cold failure");
  assert(!currentReconnect(holder.current), "cold-start failure surfaces error, not reconnecting");
  assert(holder.current?.error?.includes("Reconnecting"), "cold failure uses blocking error state");

  await act(async () => root.unmount());
});

test("same-authority refetches coalesce without aborting an in-flight read", async () => {
  const { useResilientQuery } = await import("./useResilientQuery");
  let activeReads = 0;
  let maxConcurrent = 0;
  let abortedCount = 0;
  let calls = 0;
  let releaseFirst!: (value: string[]) => void;
  let releaseSecond!: (value: string[]) => void;

  const holder: { current: UseResilientQueryResult<string[]> | null } = { current: null };
  function CoalesceProbe() {
    const state = useResilientQuery(
      (signal) => {
        calls += 1;
        activeReads += 1;
        maxConcurrent = Math.max(maxConcurrent, activeReads);
        signal.addEventListener("abort", () => { abortedCount += 1; }, { once: true });
        const call = calls;
        return new Promise<string[]>((resolve, reject) => {
          signal.addEventListener("abort", () => {
            activeReads -= 1;
            reject(new DOMException("Aborted", "AbortError"));
          }, { once: true });
          const finish = (value: string[]) => {
            activeReads -= 1;
            resolve(value);
          };
          if (call === 1) releaseFirst = finish;
          else if (call === 2) releaseSecond = finish;
          else finish([`v${call}`]);
        });
      },
      [],
      { deps: ["same-authority"] },
    );
    useEffect(() => { holder.current = state; }, [state]);
    return null;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(<CoalesceProbe />); });
  assert(calls === 1 && activeReads === 1, "initial read in flight");

  let pendingRefetches!: Promise<unknown>;
  await act(async () => {
    pendingRefetches = Promise.all([
      holder.current!.refetch(),
      holder.current!.refetch(),
      holder.current!.refetch(),
    ]);
    await Promise.resolve();
  });
  assert(calls === 1 && abortedCount === 0, "same-authority refetches do not abort initial read");

  await act(async () => {
    releaseFirst(["v1"]);
    await Promise.resolve();
    await Promise.resolve();
  });
  assert(holder.current?.data.join(",") === "v1", "initial response becomes readable before follow-up completes");
  assert(calls === 2 && maxConcurrent === 1, "single coalesced follow-up starts with concurrency 1");

  await act(async () => {
    releaseSecond(["v2"]);
    await pendingRefetches;
  });
  assert(holder.current?.data.join(",") === "v2", "converges to latest value");
  assert(calls === 2 && abortedCount === 0 && maxConcurrent === 1, "bounded concurrency with zero aborts");
  await act(async () => root.unmount());
  host.remove();
});

test("authority change and auth denial immediately fence held and retained data", async () => {
  const { useResilientQuery } = await import("./useResilientQuery");
  const firstSignal: { current: AbortSignal | null } = { current: null };
  let releaseFirst!: (value: string[]) => void;
  let mode: "hold" | "ok" | "denied" = "ok";

  const holder: { current: UseResilientQueryResult<string[]> | null } = { current: null };
  function AuthorityProbe({ token }: { token: string }) {
    const state = useResilientQuery(
      (signal) => {
        if (mode === "hold") {
          firstSignal.current = signal;
          return new Promise<string[]>((resolve) => { releaseFirst = resolve; });
        }
        if (mode === "denied") {
          return Promise.reject(Object.assign(new Error("plans list failed: 403 forbidden"), { status: 403 }));
        }
        return Promise.resolve([`data-${token}`]);
      },
      [],
      { deps: [token] },
    );
    holder.current = state;
    return <output>{state.data.join(",")}</output>;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(<AuthorityProbe token="cred-a" />); });
  assert(holder.current?.data.join(",") === "data-cred-a", "initial credential loads data");

  mode = "hold";
  await act(async () => { void holder.current!.refetch(); await Promise.resolve(); });
  assert(firstSignal.current && !firstSignal.current.aborted, "held read active under cred-a");

  mode = "ok";
  await act(async () => { root.render(<AuthorityProbe token="cred-b" />); });
  assert(firstSignal.current?.aborted, "credential change immediately aborts held read");
  assert(holder.current?.data.join(",") === "data-cred-b", "new credential publishes its own data");

  await act(async () => { releaseFirst(["data-cred-a-late"]); await Promise.resolve(); });
  assert(holder.current?.data.join(",") === "data-cred-b", "late old-credential reply cannot overwrite current data");

  mode = "denied";
  await act(async () => { await holder.current!.refetch(); });
  assert(holder.current?.data.length === 0, "403 denial clears old-authority data");
  assert(holder.current?.error?.includes("403"), "denial error detail survives");
  await act(async () => root.unmount());
  host.remove();
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
