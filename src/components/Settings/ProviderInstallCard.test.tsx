import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderCliStatus } from "../../lib/providerCli";

const fixture = vi.hoisted(() => ({
  invoke: vi.fn(), listen: vi.fn(),
  complete: null as null | ((event: { payload: { provider: string; summary: string } }) => void),
}));
vi.mock("../../lib/bridge", () => ({ invoke: fixture.invoke, listen: fixture.listen }));
import { ProviderCliControls } from "./ProviderInstallCard";

const missing: ProviderCliStatus = { required: true, installed: false, binary: "tool", resolved_path: null, availability: "unavailable", searched: [], installer: "opaque --install", updater: null };
let status = missing;
let host: HTMLDivElement;
let root: Root;
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
beforeEach(() => {
  status = missing;
  fixture.invoke.mockReset().mockImplementation(async (command: string) => command === "provider_cli_status" ? status : { tab_id: "utility", launch_state: "command_dispatched" });
  fixture.listen.mockReset().mockImplementation(async (_event: string, callback: typeof fixture.complete) => { fixture.complete = callback; return () => { fixture.complete = null; }; });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });

describe("provider CLI controls", () => {
  it("dispatches the named install control once and shows the completion model", async () => {
    await act(async () => root.render(<ProviderCliControls providerId="fixture" />));
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="Install CLI for fixture"]');
    expect(button).not.toBeNull();
    await act(async () => button!.click());
    expect(fixture.invoke.mock.calls.filter(([command]) => command === "install_provider_in_tab")).toEqual([["install_provider_in_tab", { providerId: "fixture" }]]);
    status = { ...missing, installed: true, availability: "available", resolved_path: "/tools/tool" };
    await act(async () => fixture.complete?.({ payload: { provider: "fixture", summary: "opaque result" } }));
    expect(host.querySelector('button[aria-label="Install CLI for fixture"]')).toBeNull();
    expect([...host.querySelectorAll('[role="status"]')].some((element) => element.textContent === "opaque result")).toBe(true);
  });
  it("offers no install action for chat backends", async () => {
    status = { ...missing, required: false, installed: null, binary: null, installer: null };
    await act(async () => root.render(<ProviderCliControls providerId="backend" compact />));
    expect(host.querySelector("button")).toBeNull();
  });
});
