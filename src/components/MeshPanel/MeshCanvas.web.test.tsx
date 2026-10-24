import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { MeshCanvas } from "./MeshCanvas";
import { WebAccessPanel } from "./WebAccessPanel";
import type { AppConfig } from "../../lib/types";

it("Web card exposes host-named QR, wires Open/Copy to its single link and Tailscale setup without a dialog", async () => {
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
  try {
    await act(async () => root.render(<MeshCanvas thisMachine={{ name: "Local", ip: "127.0.0.1" }} peers={[]}
      onPeerClick={() => {}} recentPushes={[]} width={390} height={700}
      webCard={(qrSize) => <WebAccessPanel embedded qrSize={qrSize} config={config} network={{ local_ip: "192.168.1.2" }}
        serve={null} loading={false} actions={actions} onAction={async () => {}} onClose={() => {}} onSetup={async () => { setup++; }} />} />));
    const card = host.querySelector('[role="region"][aria-label="Mesh UI"]');
    expect(card).not.toBeNull();
    expect(card!.querySelector('img[alt="Open Domios Web on 192.168.1.2:7686"]')).not.toBeNull();
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    const button = (name: string) => {
      const result = Array.from(card!.querySelectorAll("button")).find((element) => element.textContent === name);
      expect(result, `Accessible button ${name}`).toBeDefined();
      return result!;
    };
    await act(async () => button("Open").click());
    await act(async () => button("Copy link").click());
    await act(async () => button("Set up Tailscale with AI").click());
    expect(opened).toEqual(["http://192.168.1.2:7686/#token=secret"]);
    expect(copied).toEqual(opened);
    expect(encoded).toEqual(opened);
    expect(setup).toBe(1);
    expect(card!.textContent).not.toContain("secret");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
