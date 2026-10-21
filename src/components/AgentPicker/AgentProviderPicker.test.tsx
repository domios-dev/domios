import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentProviderPicker } from "./AgentProviderPicker";
import { applyAvailabilityChange, PROVIDER_AVAILABILITY_CHANGED } from "../../lib/providerAvailability";
import { shouldReturnToPicker, type PickerNavigation } from "./pickerNavigation";

const listeners = vi.hoisted(() => new Map<string, Array<(event: { payload: unknown }) => void>>());

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(async (command: string) => command === "install_provider_in_tab"
    ? { tab_id: "installer-tab", workspace_id: "installer-ws", launch_state: "command_dispatched" }
    : undefined),
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(event, [...(listeners.get(event) ?? []), handler]);
    return () => listeners.set(event, (listeners.get(event) ?? []).filter((h) => h !== handler));
  }),
}));

vi.mock("../AgentIcon", () => ({ AgentIcon: () => null }));

import { invoke } from "../../lib/bridge";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

afterEach(() => {
  listeners.clear();
  vi.mocked(invoke).mockClear();
});

function provider(id: string, availability: ProviderDescriptor["availability"]): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id,
    transport: "pty",
    availability,
    install_command: "opaque-installer",
    system_prompt_delivery: { cli_flag: "--system" },
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  };
}

function emit(event: string, payload: unknown) {
  for (const handler of listeners.get(event) ?? []) handler({ payload });
}

it("Install shows the installer tab, then the pushed availability flips the provider and returns home", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let active: string | null = "home-tab";
  const show = vi.fn((tab: { tabId: string }) => { active = tab.tabId; });
  const navigation: PickerNavigation = { show, activeTabId: () => active };
  try {
    await act(async () => root.render(
      <AgentProviderPicker
        headline="Pick"
        subhead="fixture"
        providers={[provider("fixture-cli", "unavailable")]}
        confirmLabel="Start"
        onConfirm={() => {}}
        navigation={navigation}
        home={{ workspaceId: "home-ws", tabId: "home-tab" }}
      />,
    ));
    const confirm = () => host.querySelector<HTMLButtonElement>("[data-testid=agent-picker-confirm]")!;
    expect(confirm().textContent).toContain("Install fixture-cli");

    await act(async () => confirm().click());
    expect(invoke).toHaveBeenCalledWith("install_provider_in_tab", { providerId: "fixture-cli" });
    expect(show).toHaveBeenLastCalledWith({ workspaceId: "installer-ws", tabId: "installer-tab" });

    await act(async () => emit(PROVIDER_AVAILABILITY_CHANGED, {
      provider: "fixture-cli", availability: "available", tab_id: "installer-tab",
    }));
    expect(confirm().textContent).toContain("Start");
    expect(show).toHaveBeenLastCalledWith({ workspaceId: "home-ws", tabId: "home-tab" });
    expect(show).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("an availability change replaces only the named provider's answer", () => {
  const providers = [provider("a", "unavailable"), provider("b", "unavailable")];
  const next = applyAvailabilityChange(providers, { provider: "a", availability: "available" });
  expect(next.map((p) => p.availability)).toEqual(["available", "unavailable"]);
  expect(applyAvailabilityChange(next, { provider: "a", availability: "available" })).toBe(next);
  expect(applyAvailabilityChange(next, { provider: "missing", availability: "available" })).toBe(next);
});

it("returns only from its own successful installer while the user is still watching it", () => {
  const ok = { provider: "a", availability: "available" as const, tab_id: "installer" };
  expect(shouldReturnToPicker(ok, "installer", "installer")).toBe(true);
  expect(shouldReturnToPicker(ok, "installer", "elsewhere")).toBe(false);
  expect(shouldReturnToPicker(ok, null, "installer")).toBe(false);
  expect(shouldReturnToPicker({ ...ok, tab_id: "other" }, "installer", "installer")).toBe(false);
  expect(shouldReturnToPicker({ ...ok, availability: "unavailable" }, "installer", "installer")).toBe(false);
});
