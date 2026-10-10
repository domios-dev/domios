import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../lib/bridge";
import { useContributions } from "./useContributions";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

async function main() {
  const win = new Window();
  Object.defineProperty(globalThis, "window", { configurable: true, value: win });
  Object.defineProperty(globalThis, "document", { configurable: true, value: win.document });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  __setListenForTest(async () => () => {});
  const pending: ReturnType<typeof deferred<string[]>>[] = [];
  const fetcher = () => {
    const next = deferred<string[]>();
    pending.push(next);
    return next.promise;
  };
  let snapshot: ReturnType<typeof useContributions<string>> | undefined;
  function Catalog() {
    snapshot = useContributions("agent_provider", fetcher);
    return null;
  }
  const host = document.createElement("main");
  const root = createRoot(host);
  const reset = () => win.dispatchEvent(new win.CustomEvent("codeterm-peer-switch-reset"));
  const rebind = () => win.dispatchEvent(new win.CustomEvent("codeterm-peer-switch-rebind"));
  try {
    await act(async () => root.render(<Catalog />));
    await act(async () => pending[0].resolve(["gateway"]));
    assert(snapshot?.items.join(",") === "gateway", "initial catalog not published");
    await act(async () => snapshot?.refetch());
    await act(async () => { reset(); });
    assert(snapshot?.loading && snapshot.items.length === 0, "reset retained gateway capabilities");
    await act(async () => { rebind(); });
    await act(async () => pending[2].resolve(["peer"]));
    assert(snapshot?.items.join(",") === "peer" && !snapshot.loading, "rebind did not load peer catalog");
    await act(async () => pending[1].resolve(["stale-gateway"]));
    assert(snapshot?.items.join(",") === "peer", "late gateway completion contaminated peer catalog");
    await act(async () => snapshot?.refetch());
    await act(async () => { reset(); rebind(); });
    await act(async () => pending[4].resolve(["next-peer"]));
    await act(async () => pending[3].reject(new Error("stale failure")));
    assert(snapshot?.items.join(",") === "next-peer" && !snapshot.error, "stale failure hid next peer catalog");
  } finally {
    await act(async () => root.unmount());
    __setListenForTest(null);
    await win.happyDOM.abort();
    win.close();
  }
  console.log("PASS contribution peer reset/rebind and stale completion isolation");
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
