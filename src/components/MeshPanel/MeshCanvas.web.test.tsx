import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { MeshCanvas } from "./MeshCanvas";

async function main() {
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Element"]) {
    Object.defineProperty(globalThis, key, { value: (window as unknown as Record<string, unknown>)[key], configurable: true });
  }
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let clicks = 0;
  const render = async (selected: boolean) => {
    await act(async () => root.render(<MeshCanvas thisMachine={{ name: "Local", ip: "127.0.0.1" }} peers={[]} recentPushes={[]} onPeerClick={() => {}} width={1000} height={700} onWebClick={() => { clicks++; }} webSelected={selected} webStatus="Unavailable" />));
  };
  await render(false);
  const web = host.querySelector<HTMLButtonElement>('button[aria-label="Web"]');
  assert.ok(web);
  assert.equal(web.getAttribute("aria-pressed"), "false");
  await act(async () => web.click());
  assert.equal(clicks, 1);
  await render(true);
  assert.equal(web.getAttribute("aria-pressed"), "true");
  await act(async () => root.unmount());
  console.log("✓ Web graph control fires selection handler once and exposes selection");
}
void main();
