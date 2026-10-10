import "../components/Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { __setListenForTest } from "../lib/bridge";
import { useContributions } from "./useContributions";

test("a superseded contribution response cannot overwrite the latest catalogue", async () => {
  const pending: Array<(value: string[]) => void> = [];
  const fetcher = () => new Promise<string[]>((resolve) => pending.push(resolve));
  let current: ReturnType<typeof useContributions<string>>;
  function Consumer() { current = useContributions("fixture", fetcher); return <span>{current.items.join(",")}</span>; }
  __setListenForTest(async () => () => undefined);
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Consumer />));
    await act(async () => current.refetch());
    await act(async () => pending[1](["new"]));
    await act(async () => pending[0](["old"]));
    assert.equal(host.textContent, "new");
  } finally { await act(async () => root.unmount()); __setListenForTest(null); }
});
