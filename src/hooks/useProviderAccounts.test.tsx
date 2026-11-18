import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { ProviderAccountsSnapshot } from "./providerAccountsTypes";

const fixture = vi.hoisted(() => ({ json: vi.fn(), handlers: new Map<string, (event: { payload: unknown }) => void>() }));
vi.mock("../lib/daemonRuntime", () => ({ selectedDaemonTransport: { json: fixture.json } }));
vi.mock("../lib/bridge", () => ({
  listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
    fixture.handlers.set(name, handler);
    return () => { if (fixture.handlers.get(name) === handler) fixture.handlers.delete(name); };
  },
}));
import { useProviderAccounts } from "./useProviderAccounts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function snapshot(providerId: string, revision: string): ProviderAccountsSnapshot {
  return { providerId, revision, accounts: [], automaticSwitch: true, continuation: "unverified", workspaceOverrides: [] };
}

async function harness(providerId: string) {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  fixture.handlers.clear();
  const host = document.createElement("div");
  const root = createRoot(host);
  let value!: ReturnType<typeof useProviderAccounts>;
  function Consumer({ provider }: { provider: string }) {
    value = useProviderAccounts(provider);
    return <output>{value.snapshot?.revision ?? "empty"}</output>;
  }
  await act(async () => root.render(<Consumer provider={providerId} />));
  return {
    get value() { return value; },
    host,
    select: async (provider: string) => { await act(async () => root.render(<Consumer provider={provider} />)); },
    close: async () => { await act(async () => root.unmount()); expect(fixture.handlers.size).toBe(0); },
  };
}

it("discards an old harness response and late subscription work after switching", async () => {
  const old = deferred<ProviderAccountsSnapshot>();
  fixture.json.mockReset().mockImplementation((_method: string, path: string) =>
    path.includes("first") ? old.promise : Promise.resolve(snapshot("second", "second-1")));
  const mounted = await harness("first");
  try {
    await mounted.select("second");
    await act(async () => old.resolve(snapshot("first", "first-9")));
    expect(mounted.host.textContent).toBe("second-1");
    expect(mounted.value.error).toBeNull();
  } finally { await mounted.close(); }
});

it("fences a stale refresh during a mutation and never replays the mutation", async () => {
  const stale = deferred<ProviderAccountsSnapshot>();
  const changed = deferred<ProviderAccountsSnapshot>();
  fixture.json.mockReset().mockResolvedValueOnce(snapshot("fixture", "one"))
    .mockReturnValueOnce(stale.promise).mockReturnValueOnce(changed.promise);
  const mounted = await harness("fixture");
  try {
    await act(async () => mounted.value.onRefresh());
    let action!: Promise<void>;
    await act(async () => {
      action = mounted.value.onAction({ action: "reorder", accountId: "work", position: 0 });
    });
    await act(async () => stale.resolve(snapshot("fixture", "stale")));
    expect(mounted.host.textContent).toBe("one");
    await act(async () => { changed.resolve(snapshot("fixture", "two")); await action; });
    expect(mounted.host.textContent).toBe("two");
    const posts = fixture.json.mock.calls.filter(([method]) => method === "POST");
    expect(posts).toHaveLength(1);
    expect(posts[0][2]).toEqual({ revision: "one", operation: { action: "reorder", accountId: "work", position: 0 } });
  } finally { await mounted.close(); }
});

it("preserves the last proven roster on a failed mutation and rejects foreign snapshots", async () => {
  fixture.json.mockReset().mockResolvedValueOnce(snapshot("fixture", "one"))
    .mockRejectedValueOnce(new Error("stale revision"))
    .mockResolvedValueOnce(snapshot("foreign", "other"));
  const mounted = await harness("fixture");
  try {
    await act(async () => {
      await expect(mounted.value.onAction({ action: "remove", accountId: "work" })).rejects.toThrow("stale revision");
    });
    expect(mounted.host.textContent).toBe("one");
    await act(async () => mounted.value.onRefresh());
    expect(mounted.host.textContent).toBe("one");
    expect(mounted.value.error).toContain("does not match this harness");
  } finally { await mounted.close(); }
});

it("an old rendered cancel cannot target a replacement sign-in attempt", async () => {
  fixture.json.mockReset().mockResolvedValueOnce({...snapshot("fixture","one"),signInId:"first"})
    .mockResolvedValueOnce({...snapshot("fixture","one"),signInId:"replacement"})
    .mockResolvedValueOnce(snapshot("fixture","one"));
  const mounted=await harness("fixture");
  try {
    const oldCancel=mounted.value.onAction;
    await act(async()=>mounted.value.onRefresh());
    await act(async()=>{await expect(oldCancel({action:"cancelAdd"})).rejects.toThrow("account list changed");});
    expect(fixture.json.mock.calls.filter(([method])=>method==="POST")).toHaveLength(0);
    await act(async()=>{await mounted.value.onAction({action:"cancelAdd"});});
    expect(fixture.json.mock.calls[fixture.json.mock.calls.length - 1]?.[2]).toEqual({revision:"one",signInId:"replacement",operation:{action:"cancelAdd"}});
  } finally {await mounted.close();}
});
