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
