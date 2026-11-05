import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { ProviderDescriptor } from "../../lib/types";
import { AcpProfilesCard } from "./AcpProfilesCard";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("../../lib/bridge", () => bridge);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const provider = { id: "acp", display_name: "ACP", agent_runtime: { launchProfiles: {
  "gemini-cli": { binary: "gemini", args: ["--acp"], install: { package: "@google/gemini-cli@0.67.0" } },
  "deepseek-harness": { binary: "npx", args: [], install: { package: "@deepseek-ai/dsh@0.2.1" } },
} } } as unknown as ProviderDescriptor;

let root: ReturnType<typeof createRoot> | null = null;
let host: HTMLDivElement;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  bridge.invoke.mockReset();
  bridge.listen.mockReset();
});

it("lists every ACP agent with its state and installs the chosen one through the profile setup job", async () => {
  bridge.listen.mockResolvedValue(() => undefined);
  const job = { jobId: "job", providerId: "acp", profileId: "deepseek-harness", state: "installing" };
  let started = false;
  bridge.invoke.mockImplementation(async (command: string) => {
    if (command !== "provider_launch_profiles") { started = true; return { ...job, state: "accepted" }; }
    return [
      { id: "gemini-cli", install: "installed", auth: "authenticated", canInstall: true, ownerConfigured: false },
      { id: "deepseek-harness", install: "not_installed", auth: "unknown", canInstall: true, ownerConfigured: false, ...(started ? { setup: job } : {}) },
    ];
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<AcpProfilesCard provider={provider} />));
  const rows = () => [...host.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
  expect(rows().map((row) => row.textContent)).toEqual(["gemini-cliReady", "deepseek-harnessNot installed"]);
  expect(host.textContent).not.toContain("No ACP agents set up yet.");

  await act(async () => rows()[1].click());
  const button = (name: string) => [...host.querySelectorAll("button")].find((entry) => entry.textContent === name)!;
  await act(async () => button("Install profile").click());
  await act(async () => host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
  await act(async () => button("Confirm setup").click());
  expect(bridge.invoke).toHaveBeenCalledWith("provider_setup_profile", { providerId: "acp", request: { profileId: "deepseek-harness", confirmed: true } });
  expect(host.querySelector('[role="status"][aria-label="Profile installation"]')?.textContent).toContain("Installing deepseek-harness");
});

it("searches the registry, installs an agent as a setup job, and shows its progress until it is a profile", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  bridge.listen.mockResolvedValue(() => undefined);
  let installing = false;
  bridge.invoke.mockImplementation(async (command: string, args: { query?: string; id?: string }) => {
    if (command === "provider_registry_search") return args.query === "qwen" ? [{ id: "qwen-code", name: "Qwen Code", version: "1.4.0", description: "Qwen's coding agent", iconUrl: null }] : [];
    if (command === "provider_registry_install") { installing = true; return { jobId: "reg", providerId: "acp", profileId: args.id, state: "accepted" }; }
    return [
      { id: "gemini-cli", install: "installed", auth: "authenticated", canInstall: true, ownerConfigured: false },
      ...(installing ? [{ id: "qwen-code", install: "not_installed", auth: "unknown", canInstall: false, ownerConfigured: false, setup: { jobId: "reg", providerId: "acp", profileId: "qwen-code", state: "installing" } }] : []),
    ];
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  try {
    await act(async () => root!.render(<AcpProfilesCard provider={provider} />));
    const search = host.querySelector<HTMLInputElement>('input[aria-label="Search the ACP registry"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(search, "qwen");
      search.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => { await vi.advanceTimersByTimeAsync(300); });
    const install = host.querySelector<HTMLButtonElement>('button[aria-label="Install Qwen Code"]');
    expect(install).not.toBeNull();
    await act(async () => install!.click());
    expect(bridge.invoke).toHaveBeenCalledWith("provider_registry_install", { providerId: "acp", id: "qwen-code" });
    expect(host.querySelector('[role="status"][aria-label="Adding qwen-code"]')).not.toBeNull();
  } finally { vi.useRealTimers(); }
});

it("removes an owner-added profile after asking once; built-in profiles offer no Remove", async () => {
  bridge.listen.mockResolvedValue(() => undefined);
  bridge.invoke.mockImplementation(async (command: string) => command === "provider_launch_profiles" ? [
    { id: "gemini-cli", install: "installed", auth: "authenticated", canInstall: true, ownerConfigured: false },
    { id: "deepseek-harness", install: "installed", auth: "unknown", canInstall: false, ownerConfigured: true },
  ] : { id: "deepseek-harness" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<AcpProfilesCard provider={provider} />));
  const rows = () => [...host.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
  const named = (name: string) => [...host.querySelectorAll("button")].filter((button) => button.textContent === name);
  await act(async () => rows()[0].click());
  expect(named("Remove")).toHaveLength(0);
  await act(async () => rows()[1].click());
  await act(async () => named("Remove")[0].click());
  expect(host.textContent).toContain("Remove deepseek-harness?");
  await act(async () => named("Remove")[0].click());
  expect(bridge.invoke).toHaveBeenCalledWith("provider_remove_profile", { providerId: "acp", name: "deepseek-harness" });
});
