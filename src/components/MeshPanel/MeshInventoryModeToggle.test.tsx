import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { MeshInventoryModeToggle, type MeshInventoryMode } from "./MeshInventoryModeToggle";

const window = new Window({ url: "http://localhost/mesh-mode" });
for (const name of ["window", "document", "HTMLElement", "HTMLButtonElement", "Node", "Element"] as const) {
  Object.defineProperty(globalThis, name, { configurable: true, value: window[name] });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Harness() {
  const [mode, setMode] = useState<MeshInventoryMode>("list");
  return <MeshInventoryModeToggle mode={mode} onChange={setMode} />;
}

async function main() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Harness />));

  const button = () => host.querySelector<HTMLButtonElement>('[data-testid="mesh-inventory-mode-toggle"]');
  if (host.querySelectorAll("button").length !== 1 || button()?.textContent !== "Graph" || button()?.dataset.inventoryMode !== "list") {
    throw new Error("list mode must expose one button leading to the existing Mesh graph");
  }
  await act(async () => button()?.click());
  if (host.querySelectorAll("button").length !== 1 || button()?.textContent !== "List" || button()?.dataset.inventoryMode !== "graph") {
    throw new Error("graph mode must expose one button leading back to the inventory list");
  }

  await act(async () => root.unmount());
  console.log("✓ Mesh inventory uses one contextual List/Graph toggle");
}

void main();
