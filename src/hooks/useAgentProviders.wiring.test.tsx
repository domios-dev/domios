import "../components/Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../lib/bridge";
import { signalEventStreamReady } from "../lib/eventStreamReady";
import { invalidateAgentProviders, useAgentProviders } from "./useAgentProviders";

test("reconnect refetches once, retains good data on failure, and accepts the restarted daemon", async () => {
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
    assert.equal(calls, 2);
    assert.equal(host.textContent, "first");
    fail = false;
    version = 1;
    name = "restarted";
    await act(async () => signalEventStreamReady());
    assert.equal(calls, 3);
    assert.equal(host.textContent, "restarted");
    name = "hot reload";
    await act(async () => handlers.get("contributions-changed")!({ payload: { kind: "agent_provider" } }));
    assert.equal(calls, 4);
    assert.equal(host.textContent, "hot reload");
  } finally {
    await act(async () => root.unmount());
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
  assert.equal(handlers.size, 0);
});

test("initial failure retries on readiness and fetch waits for contribution listener attachment", async () => {
  invalidateAgentProviders();
  let attach!: (dispose: () => void) => void;
  let calls = 0;
  __setListenForTest(async (event) => {
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
    assert.equal(calls, 2);
    assert.equal(host.textContent, "ready");
  } finally {
    await act(async () => root.unmount());
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});
