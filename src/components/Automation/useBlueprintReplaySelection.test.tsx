import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { GraphRun } from "../../lib/blueprintRun";
import { blueprintModel, type BlueprintModel } from "./blueprintModel";
import { typedGraph, watchIdentity } from "./blueprintModel.testdata";
import { runRow, type RunRow } from "./blueprintRunModel";
import { automationKey } from "./types";
import { useBlueprintReplaySelection } from "./useBlueprintReplaySelection";

async function main() {
  const window = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "HTMLElement", "Element", "Node", "navigator"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const run: GraphRun = {
    key: "run-to-hold", at: 100, name: "Build watch", state: "completed", incomplete: false,
    events: [{ at: 100, node: "Build watch", outcome: "idle", target: null, handler: null, status: "No action", duration: null, edgeId: null, stage: automationKey(watchIdentity) }],
  };
  const model = blueprintModel(typedGraph());
  const row = runRow(model, run);
  function Selection({ model, rows }: { model: BlueprintModel; rows: RunRow[] }) {
    const replay = useBlueprintReplaySelection(model, rows, run.key);
    return <>
      <output>{replay.shown?.outcome?.label ?? "none"}</output>
      <button onClick={replay.clear}>Clear</button>
      <button onClick={() => replay.replay(row)}>Replay</button>
      <span role="status">{replay.selection?.nonce ?? "none"}</span>
    </>;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Selection model={model} rows={[row]} />));
    assert.equal(host.querySelector("output")?.textContent, "Finished: idle, no action");
    await act(async () => root.render(<Selection model={blueprintModel(typedGraph())} rows={[]} />));
    assert.equal(host.querySelector("output")?.textContent, "Finished: idle, no action", "polling and run-history eviction preserve the selected result");
    const [clear, replay] = host.querySelectorAll("button");
    await act(async () => clear.click());
    assert.equal(host.querySelector("output")?.textContent, "none");
    await act(async () => replay.click());
    assert.equal(host.querySelector("output")?.textContent, "Finished: idle, no action");
    assert.equal(host.querySelector('[role="status"]')?.textContent, "1");
    await act(async () => replay.click());
    assert.equal(host.querySelector('[role="status"]')?.textContent, "2", "each click restarts the selected snapshot explicitly");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    await window.happyDOM.close();
  }
  console.log("Replay selection holds its run snapshot across graph refresh and history eviction until cleared");
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
