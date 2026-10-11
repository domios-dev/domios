import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderCliStatus } from "../../lib/providerCli";

const fixture = vi.hoisted(() => ({
  invoke: vi.fn(), listen: vi.fn(),
  complete: null as null | ((event: { payload: { provider: string; summary: string } }) => void),
}));
vi.mock("../../lib/bridge", () => ({ invoke: fixture.invoke, listen: fixture.listen }));
import { providerCompatibilityHint } from "../../lib/providerCli";
import { ProviderCliControls } from "./ProviderInstallCard";

const missing: ProviderCliStatus = { required: true, installed: false, binary: "tool", resolved_path: null, availability: "unavailable", searched: [], installer: "opaque --install", updater: null };
let status: ProviderCliStatus | Error = missing;
let host: HTMLDivElement;
let root: Root;
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
beforeEach(() => {
  status = missing;
  fixture.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command !== "provider_cli_status") return { tab_id: "utility", launch_state: "command_dispatched" };
    if (status instanceof Error) throw status;
    return status;
  });
  fixture.listen.mockReset().mockImplementation(async (_event: string, callback: typeof fixture.complete) => { fixture.complete = callback; return () => { fixture.complete = null; }; });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });

const statusText = () => host.querySelector('[role="group"] [role="status"]')?.textContent ?? "";
const button = (name: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`);
const statusCalls = () => fixture.invoke.mock.calls.filter(([command]) => command === "provider_cli_status").length;

describe("provider CLI controls", () => {
  it("loads a persisted automatic update result in Settings", async () => {
    status = { ...missing, installed: true, update_summary: "opaque persisted result" };
    await act(async () => root.render(<ProviderCliControls providerId="fixture" />));
    expect([...host.querySelectorAll('[role="status"]')].some(element => element.textContent === "opaque persisted result")).toBe(true);
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });
  it("renders the model's missing prerequisite guidance in a status", async () => {
    status = { ...missing, installer_prerequisites: [{ binary: "runner", installed: false, hint: "opaque setup guidance" }] };
    await act(async () => root.render(<ProviderCliControls providerId="fixture" />));
    expect([...host.querySelectorAll('[role="status"]')].some(element => element.textContent?.includes("opaque setup guidance"))).toBe(true);
  });
  it("renders unverified compatibility in a quiet accessible Settings status", async () => {
    status = { ...missing, installed: true, detected_version: "1.20.0", compatibility: { verdict: "unknown", reasons: ["version_not_declared"] } };
    await act(async () => root.render(<ProviderCliControls providerId="fixture" />));
    expect(host.querySelector('[role="status"][aria-label="CLI compatibility"]')?.textContent).toBe(providerCompatibilityHint(status));
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });
  it("shows a missing CLI's install command once in a code block beside the Install action", async () => {
    await act(async () => root.render(<ProviderCliControls providerId="fixture" />));
    const region = host.querySelector('[role="region"][aria-label="Install command"]');
    expect(region?.textContent).toContain("opaque --install");
    expect(host.textContent?.split("opaque --install").length).toBe(2);
    expect(button("Install CLI for fixture")).not.toBeNull();
    expect(button("Re-check CLI for fixture")).toBeNull();
  });
  it("an installed CLI is one status row with its version and no actions or command", async () => {
    status = { ...missing, installed: true, availability: "available", resolved_path: "/opaque/tool", detected_version: "9.8.7" };
    await act(async () => root.render(<ProviderCliControls providerId="fixture" />));
    expect(statusText()).toContain("9.8.7");
    expect(host.textContent).not.toContain("/opaque/tool");
    expect(host.querySelector('[title="/opaque/tool"]')).not.toBeNull();
    expect(host.querySelector('[aria-label="Install command"]')).toBeNull();
    expect(host.querySelector("button")).toBeNull();
  });
  it("an unknown status offers a re-check that queries the CLI again", async () => {
    status = new Error("opaque probe failure");
    await act(async () => root.render(<ProviderCliControls providerId="fixture" />));
    const recheck = button("Re-check CLI for fixture");
    expect(recheck).not.toBeNull();
    expect(host.querySelector('[aria-label="Install command"]')).toBeNull();
    const before = statusCalls();
    status = { ...missing, installed: true, availability: "available" };
    await act(async () => recheck!.click());
    expect(statusCalls()).toBe(before + 1);
    expect(button("Re-check CLI for fixture")).toBeNull();
  });
  it("dispatches the named install control once and shows the completion model", async () => {
    await act(async () => root.render(<ProviderCliControls providerId="fixture" />));
    const install = button("Install CLI for fixture");
    expect(install).not.toBeNull();
    await act(async () => install!.click());
    expect(fixture.invoke.mock.calls.filter(([command]) => command === "install_provider_in_tab")).toEqual([["install_provider_in_tab", { providerId: "fixture" }]]);
    status = { ...missing, installed: true, availability: "available", resolved_path: "/tools/tool" };
    await act(async () => fixture.complete?.({ payload: { provider: "fixture", summary: "opaque result" } }));
    expect(button("Install CLI for fixture")).toBeNull();
    expect([...host.querySelectorAll('[role="status"]')].some((element) => element.textContent === "opaque result")).toBe(true);
  });
  it("the compact row never carries the install command", async () => {
    await act(async () => root.render(<ProviderCliControls providerId="fixture" compact />));
    expect(button("Install CLI for fixture")).not.toBeNull();
    expect(host.querySelector('[aria-label="Install command"]')).toBeNull();
  });
  it("offers no install action for chat backends", async () => {
    status = { ...missing, required: false, installed: null, binary: null, installer: null };
    await act(async () => root.render(<ProviderCliControls providerId="backend" compact />));
    expect(host.querySelector("button")).toBeNull();
  });
});
