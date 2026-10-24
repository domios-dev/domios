import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { MultiplexerState, ProviderDescriptor, Tab } from "../../lib/types";
import { FirstRunGeneralPicker } from "./FirstRunGeneralPicker";
import { firstRunGeneralTarget } from "./firstRunNavigation";

const navigation = vi.hoisted(() => ({ show: vi.fn(), activeTabId: () => "installer" }));

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(async () => undefined),
  listen: vi.fn(async () => () => {}),
}));

vi.mock("./pickerNavigation", async (original) => ({
  ...await original<typeof import("./pickerNavigation")>(),
  usePickerNavigation: () => navigation,
}));

import { invoke } from "../../lib/bridge";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

afterEach(() => {
  vi.mocked(invoke).mockReset();
  navigation.show.mockReset();
});

function snapshot(): MultiplexerState {
  return {
    tabs: {
      installer: { id: "installer", title: "CliInstall" } as Tab,
      general: { id: "general", role: "Manager", role_profile: "general_agent", protected: true } as Tab,
    },
    workspaces: [{ id: "welcome", name: "Welcome", cwd: null, tabs: ["installer", "general"], active_tab_id: "installer" }],
    active_workspace_id: "welcome", focused_tab_id: "installer",
  };
}

it("selects the protected General by identity beside the finished installer", () => {
  expect(firstRunGeneralTarget(snapshot())).toEqual({ workspaceId: "welcome", tabId: "general" });
});

it("refuses hidden, unprotected and unattached General candidates", () => {
  for (const patch of [{ hidden: true }, { show_in_tabbar: false }, { protected: false }, { role_profile: null }]) {
    const state = snapshot();
    Object.assign(state.tabs.general, patch);
    expect(firstRunGeneralTarget(state)).toBeNull();
  }
  const state = snapshot();
  state.workspaces[0].tabs = ["installer"];
  expect(firstRunGeneralTarget(state)).toBeNull();
});

function provider(id: string, transport: ProviderDescriptor["transport"], displayName = id): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: displayName,
    transport,
    agent_runtime: transport === "chat_backend" ? {} : undefined,
    availability: "available",
    system_prompt_delivery: { cli_flag: "--system" },
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  };
}

it("Welcome offers Codex's runtime and confirms the selected provider once", async () => {
  vi.mocked(invoke).mockImplementation(async (command) => command === "mux_get_state" ? snapshot() : undefined);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<FirstRunGeneralPicker providers={[
      provider("terminal-agent", "pty"),
      provider("codex", "chat_backend", "Codex"),
    ]} />));
    const buttons = () => Array.from(host.querySelectorAll("button"));
    const codex = buttons().find((button) => button.textContent?.includes("Codex"));
    expect(codex).toBeDefined();
    await act(async () => codex!.click());
    expect(codex!.getAttribute("aria-pressed")).toBe("true");
    const confirm = buttons().find((button) => button.textContent?.trim() === "Start General agent");
    expect(confirm?.disabled).toBe(false);
    await act(async () => confirm!.click());
    expect(invoke).toHaveBeenCalledWith("complete_general_agent_first_run", {
      providerId: "codex", fullAccess: true,
    });
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "complete_general_agent_first_run")).toHaveLength(1);
    expect(navigation.show).toHaveBeenCalledExactlyOnceWith({ workspaceId: "welcome", tabId: "general" });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("keeps the picker visible and does not focus a tab when setup fails", async () => {
  vi.mocked(invoke).mockRejectedValue("opaque setup failure");
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<FirstRunGeneralPicker providers={[provider("terminal-agent", "pty")]} />));
    const confirm = host.querySelector<HTMLButtonElement>("[data-testid=agent-picker-confirm]");
    await act(async () => confirm!.click());
    expect(navigation.show).not.toHaveBeenCalled();
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual(["complete_general_agent_first_run"]);
    expect(host.textContent).toContain("opaque setup failure");
    expect(confirm?.disabled).toBe(false);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
