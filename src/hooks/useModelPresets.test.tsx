import "../components/Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../lib/bridge";
import { useModelPresets } from "./useModelPresets";

test("catalogue events refresh visible preset choices and dispose subscriptions", async () => {
  let names = ["demo"];
  let calls = 0;
  let disposed = 0;
  const handlers = new Map<string, (event: { payload: unknown }) => void>();
  __setInvokeForTest(async <T,>(command: string) => {
    assert.equal(command, "model_provider_manage");
    calls++;
    return { provider: "fixture", presets: names.map((name) => ({ name, model: "endpoint/model" })) } as T;
  });
  __setListenForTest(async (event, handler) => {
    handlers.set(event, handler as (event: { payload: unknown }) => void);
    return () => { disposed++; handlers.delete(event); };
  });
  function Consumer() {
    const { presets } = useModelPresets();
    return <span>{presets.map((preset) => `${preset.provider}/${preset.name}`).join(",")}</span>;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Consumer />));
    assert.equal(host.textContent, "fixture/demo");
    names = ["replacement"];
    await act(async () => handlers.get("model-catalogue-changed")!({ payload: { provider: "fixture", version: 100 } }));
    assert.equal(host.textContent, "fixture/replacement");
    assert.equal(calls, 2);
  } finally {
    await act(async () => root.unmount());
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
  assert.equal(disposed, 2);
  assert.equal(handlers.size, 0);
});
