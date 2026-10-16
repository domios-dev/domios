import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { MeshCanvas } from "./MeshCanvas";
import { WebAccessPanel } from "./WebAccessPanel";
import type { AppConfig } from "../../lib/types";

async function main() {
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "ResizeObserver"]) {
    Object.defineProperty(globalThis, key, { value: (window as unknown as Record<string, unknown>)[key], configurable: true });
  }
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const opened: string[] = [], copied: string[] = [], encoded: string[] = [];
  let setup = 0;
  const actions = {
    open: (link: string) => { opened.push(link); },
    copy: async (link: string) => { copied.push(link); },
    qr: async (link: string) => { encoded.push(link); return "data:image/png;base64,fixture"; },
  };
  const config = { api: { port: 7686, token: "secret" } } as AppConfig;
  await act(async () => root.render(<MeshCanvas thisMachine={{ name: "Local", ip: "127.0.0.1" }} peers={[]}
    onPeerClick={() => {}} recentPushes={[]} width={390} height={700}
    webCard={(qrSize) => <WebAccessPanel embedded qrSize={qrSize} config={config} network={{ local_ip: "192.168.1.2" }}
      serve={null} loading={false} actions={actions} onAction={async () => {}} onClose={() => {}} onSetup={async () => { setup++; }} />} />));
  const card = host.querySelector('[role="region"][aria-label="Web"]');
  assert.ok(card);
  assert.ok(card.querySelector('img[alt="Open Domios Web on 192.168.1.2:7686"]'));
  assert.equal(host.querySelector('[role="dialog"]'), null);
  const button = (name: string) => {
    const result = Array.from(card.querySelectorAll("button")).find((element) => element.textContent === name);
    assert.ok(result, `Accessible button ${name}`);
    return result;
  };
  await act(async () => button("Open").click());
  await act(async () => button("Copy link").click());
  await act(async () => button("Set up Tailscale with AI").click());
  assert.deepEqual(opened, ["http://192.168.1.2:7686/#token=secret"]);
  assert.deepEqual(copied, opened);
  assert.deepEqual(encoded, opened);
  assert.equal(setup, 1);
  assert.ok(!card.textContent?.includes("secret"));
  await act(async () => root.unmount());
  console.log("✓ Web card exposes host-named QR, wires Open/Copy to its single link and Tailscale setup without a dialog");
}
void main();
