import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import assert from "node:assert/strict";
import type { AppConfig } from "../../lib/types";
import { webUiSnapshot } from "./webUiFixture.testdata";
import { deriveWebUiView } from "./meshWebUi";
import { WebAccessPanel, type WebUiAction } from "./WebAccessPanel";

async function main() {
const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "navigator", "HTMLElement"]) {
  Object.defineProperty(globalThis, key, { value: (window as unknown as Record<string, unknown>)[key], configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);
const copied: string[] = [];
const opened: string[] = [];
const encoded: string[] = [];
const mutations: WebUiAction[] = [];
let setupCount = 0;
let closeCount = 0;
const config = { api: { port: 7686, token: "opaque-secret" } } as AppConfig;
const actions = {
  copy: async (link: string) => { copied.push(link); },
  open: (link: string) => { opened.push(link); },
  qr: async (link: string) => { encoded.push(link); return "data:image/png;base64,fixture"; },
};
await act(async () => root.render(<WebAccessPanel config={config} network={{ local_ip: "192.168.1.2" }} serve={null} loading={false} actions={actions} onAction={async (action) => { mutations.push(action); }} onSetup={async () => { setupCount++; }} onClose={() => { closeCount++; }} />));
const button = (name: string) => {
  const element = Array.from(host.querySelectorAll("button")).find((entry) => (entry.getAttribute("aria-label") ?? entry.textContent) === name);
  assert.ok(element, `button ${name} exists`);
  return element;
};
assert.ok(host.querySelector('[role="dialog"][aria-labelledby="web-access-heading"]'));
assert.ok(host.querySelector('img[alt="Open Domios Web on 192.168.1.2:7686"]'));
await act(async () => button("Open").click());
await act(async () => button("Copy link").click());
assert.deepEqual(opened, [encoded[0]]);
assert.deepEqual(copied, [encoded[0]]);
assert.ok(copied[0].includes("opaque-secret"));
assert.equal(host.querySelector<HTMLInputElement>('input[aria-label="Web address"]')?.value, "http://192.168.1.2:7686/");
await act(async () => button("Set up Tailscale with AI").click());
assert.equal(setupCount, 1);
await act(async () => button("Close Mesh").click());
assert.equal(closeCount, 1);
assert.equal(mutations.length, 0);
for (const state of ["ready", "setup_required", "conflict"] as const) {
  const snapshot = webUiSnapshot(state);
  await act(async () => root.render(<WebAccessPanel config={config} network={null} serve={{installed:true,active:state==="ready",snapshot}} loading={false} actions={actions} onAction={async (action) => { mutations.push(action); }} onSetup={async () => { setupCount++; }} onClose={() => { closeCount++; }} />));
  assert.equal(host.querySelector('[data-testid="web-access-status"]')?.textContent, deriveWebUiView(snapshot, false).label);
  const name = state === "ready" ? "Disable Web UI" : state === "conflict" ? "Repair route" : "Enable HTTPS (Tailscale Serve)";
  await act(async () => button(name).click());
}
assert.deepEqual(mutations, ["disable", "setup", "repair"]);
await act(async () => root.unmount());
console.log("✓ Web panel wires the resolved link to QR, Copy and Open and retains setup/close actions");

}
void main();
