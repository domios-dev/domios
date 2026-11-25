import "../components/Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../lib/bridge";
import { signalEventStreamReady } from "../lib/eventStreamReady";
import { invalidateAgentProviders, useAgentProviders } from "./useAgentProviders";
import { clearProviderProbes, probeProvider } from "../lib/providerAvailabilityProbe";
import type { ProviderCliStatus } from "../lib/providerCli";

test("stream readiness does not refetch; provider changes do and failures retain good data", async () => {
  invalidateAgentProviders();
  let calls = 0;
  let fail = false;
  let name = "first";
  let version = 10;
  const handlers = new Map<string, (event: { payload: unknown }) => void>();
  __setInvokeForTest(async <T,>(command: string) => {
    assert.equal(command, "list_agent_providers");
    calls++;
    if (fail) throw new Error("offline");
    return [{ id: "fixture", display_name: name, catalogue_version: version }] as T;
  });
  __setListenForTest(async (event, handler) => {
    handlers.set(event, handler as (event: { payload: unknown }) => void);
    return () => { if (handlers.get(event) === handler) handlers.delete(event); };
  });
  function Consumer() {
    const { providers } = useAgentProviders();
    return <output>{providers.map((provider) => provider.display_name).join(",")}</output>;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<StrictMode><Consumer /></StrictMode>));
    assert.equal(calls, 1);
    assert.equal(host.textContent, "first");
    fail = true;
    await act(async () => signalEventStreamReady());
    assert.equal(calls, 1, "reconnect readiness does not invalidate the provider catalogue");
    assert.equal(host.textContent, "first");
    fail = false;
    version = 11;
    name = "updated";
    await act(async () => handlers.get("contributions-changed")!({ payload: { kind: "agent_provider" } }));
    assert.equal(calls, 2);
    assert.equal(host.textContent, "updated");
  } finally {
    await act(async () => root.unmount());
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
  assert.equal(handlers.size, 0);
});

test("initial failure waits for a server change and fetch waits for listener attachment", async () => {
  invalidateAgentProviders();
  let attach!: (dispose: () => void) => void;
  let calls = 0;
  const handlers = new Map<string, (event: { payload: unknown }) => void>();
  __setListenForTest(async (event, handler) => {
    handlers.set(event, handler as (event: { payload: unknown }) => void);
    if (event === "contributions-changed") return new Promise<() => void>((resolve) => { attach = resolve; });
    return () => {};
  });
  __setInvokeForTest(async <T,>() => {
    calls++;
    if (calls === 1) throw new Error("boot busy");
    return [{ id: "fixture", display_name: "ready" }] as T;
  });
  function Consumer() {
    const { providers } = useAgentProviders();
    return <output>{providers.map((provider) => provider.display_name).join(",")}</output>;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Consumer />));
    assert.equal(calls, 0);
    await act(async () => attach(() => {}));
    assert.equal(calls, 1);
    await act(async () => signalEventStreamReady());
    assert.equal(calls, 1, "readiness alone does not cause a retry");
    await act(async () => handlers.get("contributions-changed")!({ payload: { kind: "agent_provider" } }));
    assert.equal(calls, 2);
    assert.equal(host.textContent, "ready");
  } finally {
    await act(async () => root.unmount());
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});

test("remote peer providers probe CLI status for that peer and settle cards out of checking", async () => {
  clearProviderProbes();
  const probeCalls: Array<Record<string, unknown> | undefined> = [];
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    if (command === "list_agent_providers") {
      assert.equal(args?.peerId, "r161");
      assert.equal(args?.metadataOnly, true);
      return [
        { id: "provider-a", display_name: "Provider A", availability: "unknown", availability_checking: true },
        { id: "provider-b", display_name: "Provider B", availability: "unknown", availability_checking: true },
      ] as T;
    }
    if (command === "provider_cli_status") {
      probeCalls.push(args);
      if (args?.providerId === "provider-a") return { availability: "available" } as ProviderCliStatus as T;
      throw new Error("relay probe failed");
    }
    throw new Error(`unexpected command: ${command}`);
  });
  function RemoteConsumer() {
    const { providers } = useAgentProviders("r161");
    return <output>{providers.map((p) => `${p.id}:${p.availability_checking ? "checking" : p.availability}`).join(",")}</output>;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RemoteConsumer />));
    assert.deepEqual(probeCalls, [
      { providerId: "provider-a", peerId: "r161" },
      { providerId: "provider-b", peerId: "r161" },
    ]);
    assert.equal(host.textContent, "provider-a:available,provider-b:unknown");
  } finally {
    await act(async () => root.unmount());
    __setInvokeForTest(null);
    clearProviderProbes();
  }
});

test("probeProvider settles a hanging probe within its bound and allows retry afterwards", async () => {
  clearProviderProbes();
  let attempt = 0;
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    assert.equal(command, "provider_cli_status");
    assert.equal(args?.providerId, "stalled");
    assert.equal(args?.peerId, "peer-a");
    attempt++;
    if (attempt === 1) return new Promise<T>(() => {});
    return { availability: "available" } as ProviderCliStatus as T;
  });
  try {
    const timedOut = await probeProvider("stalled", "peer-a", 15);
    assert.equal(timedOut, null);
    const recovered = await probeProvider("stalled", "peer-a", 100);
    assert.equal(recovered?.availability, "available");
    assert.equal(attempt, 2);
  } finally {
    __setInvokeForTest(null);
    clearProviderProbes();
  }
});
