import { act } from "react";
import { createRoot } from "react-dom/client";
import { it, expect } from "vitest";
import assert from "node:assert/strict";
import type { AppConfig } from "../../lib/types";
import { webUiSnapshot } from "./webUiFixture.testdata";
import { deriveWebUiView } from "./meshWebUi";
import { WebAccessPanel, type WebUiAction } from "./WebAccessPanel";

it("shows links only for proven listeners, including while account mesh is signed out", async () => {
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
assert.equal(host.querySelector('img[alt^="Open Domios Web"]'), null);
assert.equal(host.querySelector('input[aria-label="Web address"]'), null);
expect(Array.from(host.querySelectorAll("button")).map(entry => entry.textContent)).not.toContain("Open");
assert.equal(encoded.length, 0);
await act(async () => root.render(<WebAccessPanel config={config} network={{ tailscale_ip: "100.64.1.2", web_listener: "100.64.1.2:7686" }} serve={null} loading={false} actions={actions} onAction={async (action) => { mutations.push(action); }} onSetup={async () => { setupCount++; }} onClose={() => { closeCount++; }} />));

assert.ok(host.querySelector('img[alt="Open Domios Web on 100.64.1.2:7686"]'));
await act(async () => button("Open").click());
await act(async () => button("Copy link").click());
assert.deepEqual(opened, ["http://100.64.1.2:7686/#token=opaque-secret&relay=self"]);
assert.deepEqual(copied, [encoded[0]]);
assert.ok(copied[0].includes("opaque-secret"));
assert.equal(host.querySelector<HTMLInputElement>('input[aria-label="Web address"]')?.value, "http://100.64.1.2:7686/");
await act(async () => button("Set up Tailscale with AI").click());
assert.equal(setupCount, 1);
await act(async () => button("Collapse Mesh UI").click());
assert.equal(closeCount, 1);
assert.equal(mutations.length, 0);
for (const state of ["ready", "setup_required", "conflict"] as const) {
  const snapshot = webUiSnapshot(state);
  await act(async () => root.render(<WebAccessPanel config={config} network={state === "ready" ? { web_listener: "100.64.1.2:7686" } : null} serve={{installed:true,active:state==="ready",snapshot}} loading={false} actions={actions} onAction={async (action) => { mutations.push(action); }} onSetup={async () => { setupCount++; }} onClose={() => { closeCount++; }} />));
  if (state !== "ready") assert.equal(host.querySelector('[data-testid="web-access-status"]')?.textContent, deriveWebUiView(snapshot, false).label);
  if (state === "ready") {
    assert.equal(Array.from(host.querySelectorAll("button")).some((entry) => entry.textContent === "Disable Web UI"), false);
    assert.ok(host.querySelector('img[alt="Open Domios Web on 100.64.1.2:7686"]'));
  } else {
    assert.equal(host.querySelector('img[alt^="Open Domios Web"]'), null);
    assert.equal(host.querySelector('input[aria-label="Web address"]'), null);
    expect(Array.from(host.querySelectorAll("button")).map(entry => entry.textContent)).not.toContain("Copy link");
    const name = state === "conflict" ? "Repair route" : "Enable HTTPS (Tailscale Serve)";
    await act(async () => button(name).click());
  }
}
assert.deepEqual(mutations, ["setup", "repair"]);
await act(async () => root.render(<WebAccessPanel config={config} network={{ transport: { display_name: "Tailscale", state: "not_running", identity: null, network_name: null, addresses: [], peer_listener: false, encrypted: true, setup_actions: [] } }} serve={{ installed: true, active: true, snapshot: webUiSnapshot("ready") }} loading={false} actions={actions} onAction={async () => {}} onSetup={async () => {}} onClose={() => {}} />));
assert.equal(host.querySelector('img[alt^="Open Domios Web"]'), null);
expect(Array.from(host.querySelectorAll("button")).map(entry => entry.textContent)).not.toContain("Open");

await act(async () => root.render(<WebAccessPanel config={config} network={null} serve={null} loading actions={actions} onAction={async () => {}} onSetup={async () => {}} onClose={() => {}} />));
assert.ok(host.querySelector('[role="status"][aria-label="Loading"]'));
assert.equal(host.querySelector('[data-testid="web-access-status"]'), null);
await act(async () => root.unmount());
});
