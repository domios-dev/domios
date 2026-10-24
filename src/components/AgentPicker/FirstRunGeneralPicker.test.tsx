import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { MultiplexerState, ProviderDescriptor, Tab, TabBootStageEvent } from "../../lib/types";
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

import { invoke, listen } from "../../lib/bridge";
import { defaultConfig, useConfigLoader } from "../../hooks/useConfig";
import { firstRunLocked } from "../../lib/firstRunGate";
import { FirstRunControls } from "./FirstRunControls";

const events = new Map<string, (event: { payload: unknown }) => void>();
beforeEach(() => {
  vi.mocked(listen).mockImplementation(async (name, handler) => {
    events.set(name, handler as (event: { payload: unknown }) => void);
    return () => { events.delete(name); };
  });
});

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

afterEach(() => {
  vi.mocked(invoke).mockReset();
  navigation.show.mockReset();
  events.clear();
});

for (const stage of ["agent_ready", "spawn_failed"] as const) {
  it(`releases navigation on persisted completion and leaves Working on ${stage}`, async () => {
    const config = defaultConfig();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_config") return config;
      if (command === "complete_general_agent_first_run") {
        events.get("config-changed")!({ payload: { ...config, general_agent: { ...config.general_agent, first_run_complete: true } } });
      }
      return command === "mux_get_state" ? snapshot() : undefined;
    });
    function Shell() {
      const { config } = useConfigLoader();
      return <><FirstRunControls locked={firstRunLocked(config.general_agent)}><button>Mesh</button></FirstRunControls>
        <FirstRunGeneralPicker providers={[provider("terminal-agent", "pty")]} /></>;
    }
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<Shell />));
      const mesh = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Mesh")!;
      const confirm = host.querySelector<HTMLButtonElement>("[data-testid=agent-picker-confirm]")!;
      expect(mesh.disabled).toBe(true);
      await act(async () => confirm.click());
      expect(confirm.disabled).toBe(true);
      expect(mesh.disabled).toBe(false);
      await act(async () => events.get("tab-boot-stage")!({ payload: { tab_id: "unrelated", stage } satisfies TabBootStageEvent }));
      expect(confirm.disabled).toBe(true);
      await act(async () => events.get("tab-boot-stage")!({ payload: { tab_id: "general", stage, reason: "opaque launch error" } satisfies TabBootStageEvent }));
      expect(confirm.disabled).toBe(false);
      expect(mesh.disabled).toBe(false);
      expect(host.textContent?.includes("opaque launch error")).toBe(stage === "spawn_failed");
      expect(events.has("tab-boot-stage")).toBe(false);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}

it("observes readiness from the accepted snapshot when the ready event has already passed", async () => {
  const ready = snapshot();
  ready.tabs.general.boot_stage = "agent_ready";
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "complete_general_agent_first_run") {
      events.get("tab-boot-stage")!({ payload: { tab_id: "general", stage: "agent_launching" } satisfies TabBootStageEvent });
    }
    return command === "mux_get_state" ? ready : undefined;
  });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<FirstRunGeneralPicker providers={[provider("terminal-agent", "pty")]} />));
    const confirm = host.querySelector<HTMLButtonElement>("[data-testid=agent-picker-confirm]")!;
    await act(async () => confirm.click());
    expect(confirm.disabled).toBe(false);
    expect(navigation.show).toHaveBeenCalledExactlyOnceWith({ workspaceId: "welcome", tabId: "general" });
    expect(events.has("tab-boot-stage")).toBe(false);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
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
