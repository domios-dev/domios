import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { MultiplexerState, ProviderDescriptor, Tab, TabBootStageEvent } from "../../lib/types";
import { FirstRunGeneralPicker } from "./FirstRunGeneralPicker";
import { firstRunGeneralTarget } from "./firstRunNavigation";
import { firstRunTransition, type FirstRunPhase } from "../../lib/firstRunFlow";

const localJson = vi.hoisted(() => vi.fn());
vi.mock("../../lib/webApi", async original => ({ ...await original<typeof import("../../lib/webApi")>(), localJson }));

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
it("models installation, sign-in, startup, readiness and failure recovery", () => {
  let phase: FirstRunPhase = "choosing";
  const phases = [];
  for (const event of ["install", "installed", "authenticated", "ready"] as const) {
    phase = firstRunTransition(phase, event);
    phases.push(phase);
  }
  expect(phases).toEqual(["installing", "signIn", "starting", "ready"]);
  expect(firstRunTransition("installing", "ready")).toBe("installing");
  expect(firstRunTransition("installing", "failure")).toBe("failed");
  expect(firstRunTransition("failed", "install")).toBe("installing");
});
beforeEach(() => {
  localJson.mockReset().mockResolvedValue({ consent: "on", queued: 0, has_device_id: true, has_user_id: false });
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

it("lays the Welcome out from the visible providers only, on one centred axis, for one to three providers", async () => {
  vi.mocked(invoke).mockImplementation(async (command) => command === "mux_get_state" ? snapshot() : undefined);
  // Offered when absent (Codex's descriptor) shows uninstalled; without that flag (Cursor) an uninstalled one is hidden.
  const absent = (id: string, offered: boolean): ProviderDescriptor => ({ ...provider(id, "pty"), availability: "unavailable",
    install_command: "opaque", ...(offered ? { general_agent_offer_when_absent: true } : {}) });
  const cases: Array<{ providers: ProviderDescriptor[]; shown: string[] }> = [
    { providers: [provider("claude", "pty"), absent("cursor", false)], shown: ["claude"] },
    { providers: [provider("claude", "pty"), absent("cursor", false), absent("codex", true)], shown: ["claude", "codex"] },
    { providers: [provider("opencode", "pty"), provider("claude", "pty"), absent("cursor", false), absent("codex", true)], shown: ["opencode", "claude", "codex"] },
  ];
  for (const { providers, shown } of cases) {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<FirstRunGeneralPicker providers={providers} />));
      const cards = host.querySelector<HTMLElement>("[data-testid=agent-picker-cards]")!;
      expect(Array.from(cards.children).map((card) => card.getAttribute("data-testid"))).toEqual(shown.map((id) => `agent-picker-card-${id}`));
      // The header, the card row and the confirm are all laid out by one group, so they share its centre.
      const group = host.querySelector<HTMLElement>("[data-testid=agent-picker-group]")!;
      expect(cards.parentElement).toBe(group);
      expect(host.querySelector("h1")?.closest("[data-testid=agent-picker-group] > *")?.parentElement).toBe(group);
      expect(host.querySelector("[data-testid=agent-picker-confirm]")?.closest("[data-testid=agent-picker-group] > *")?.parentElement).toBe(group);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  }
});

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

for (const success of [true, false]) {
  it(`continues only its own verified installer completion (success=${success})`, async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "install_provider_in_tab") return { tab_id: "own-install", launch_state: "command_dispatched" };
      if (command === "mux_get_state") return snapshot();
    });
    const missing = { ...provider("fixture", "pty"), availability: "unavailable" as const, general_agent_offer_when_absent: true, install_command: "opaque" };
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<FirstRunGeneralPicker providers={[missing]} />));
      const confirm = () => host.querySelector<HTMLButtonElement>("[data-testid=agent-picker-confirm]")!;
      await act(async () => confirm().click());
      expect(confirm().disabled).toBe(true);
      expect(host.querySelector('[role="status"]')?.textContent).toContain("Installing");
      expect(navigation.show).not.toHaveBeenCalled();
      const completion = { provider: "fixture", operation: "install", tab_id: "other-install", success: true, summary: "unrelated" };
      await act(async () => events.get("provider-cli-completed")!({ payload: completion }));
      expect(invoke).not.toHaveBeenCalledWith("complete_general_agent_first_run", expect.anything());
      await act(async () => events.get("provider-cli-completed")!({ payload: { ...completion, tab_id: "own-install", success, summary: "opaque installer failure" } }));
      if (success) {
        expect(invoke).toHaveBeenCalledWith("complete_general_agent_first_run", { providerId: "fixture", fullAccess: true });
        expect(navigation.show).toHaveBeenCalledExactlyOnceWith({ workspaceId: "welcome", tabId: "general" });
      } else {
        expect(invoke).not.toHaveBeenCalledWith("complete_general_agent_first_run", expect.anything());
        expect(host.textContent).toContain("opaque installer failure");
        expect(confirm().disabled).toBe(false);
      }
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}

it("keeps multi-method sign-in on Get started and continues only authenticated completion", async () => {
  const signedOut = { state: "loginPending", epoch: 0, auth: "signedOut", loginMethods: ["browser"],
    loginChoices: [{ id: "plan", label: "Plan account" }, { id: "metered", label: "Usage account" }] };
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "agent_runtime_login") return { state: signedOut, login: { status: "pending", loginId: "own-login" } };
    if (command === "mux_get_state") return snapshot();
  });
  const available = { ...provider("fixture", "pty"), cli_login: { methods: [], urlPattern: "", timeoutMs: 1000 } };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<FirstRunGeneralPicker providers={[available]} home={{ workspaceId: "welcome", tabId: "home" }} />));
    await act(async () => host.querySelector<HTMLButtonElement>("[data-testid=agent-picker-confirm]")!.click());
    expect(invoke).not.toHaveBeenCalledWith("complete_general_agent_first_run", expect.anything());
    const method = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Usage account")!;
    await act(async () => method.click());
    expect(invoke).toHaveBeenCalledWith("agent_runtime_login", { tabId: "home", request: { method: "browser", account: "metered", providerId: "fixture" } });
    const completed = { state: { ...signedOut, state: "loaded", auth: "signedIn" }, session_binding_nonce: null, tab_id: "other" };
    await act(async () => events.get("agent-runtime-state")!({ payload: completed }));
    await act(async () => events.get("agent-runtime-state")!({ payload: { ...completed, tab_id: "home", session_binding_nonce: "foreign-nonce" } }));
    expect(invoke).not.toHaveBeenCalledWith("complete_general_agent_first_run", expect.anything());
    await act(async () => events.get("agent-runtime-state")!({ payload: { ...completed, tab_id: "home" } }));
    expect(invoke).toHaveBeenCalledWith("complete_general_agent_first_run", { providerId: "fixture", fullAccess: true });
    expect(navigation.show).toHaveBeenCalledExactlyOnceWith({ workspaceId: "welcome", tabId: "general" });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("shows only the passive analytics line on first run and links Settings to General", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const open = vi.fn();
  window.addEventListener("codeterm:open-panel", open);
  vi.mocked(invoke).mockResolvedValue(undefined);
  try {
    await act(async () => root.render(<FirstRunGeneralPicker providers={[]} />));
    const link = [...host.querySelectorAll("a")].find(element => element.textContent === "Settings")!;
    expect(link.parentElement?.textContent).toBe("Product analytics is on. Turn off in Settings.");
    expect(host.querySelector('[role="dialog"], [role="alert"], [role="banner"], [data-notification-id]')).toBeNull();
    await act(async () => link.click());
    expect(open).toHaveBeenCalledOnce();
    expect((open.mock.calls[0][0] as CustomEvent).detail).toEqual({ panel: "settings:general" });
    const { AgentProviderPicker } = await import("./AgentProviderPicker");
    await act(async () => root.render(<AgentProviderPicker headline="Choose agent" subhead="" providers={[]} onConfirm={() => {}} />));
    expect(host.textContent).not.toContain("Product analytics is on.");
    expect(host.querySelector('[role="dialog"], [role="alert"], [role="banner"], [data-notification-id]')).toBeNull();
  } finally {
    window.removeEventListener("codeterm:open-panel", open);
    await act(async () => root.unmount()); host.remove();
  }
});


it.each(["off", "unknown"] as const)("hides the first-run analytics line when actual consent is %s", async consent => {
  if (consent === "off") localJson.mockResolvedValue({ consent: "off" });
  else localJson.mockRejectedValue(new Error("status unavailable"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<FirstRunGeneralPicker providers={[]} />));
    expect(host.textContent).not.toContain("Product analytics is on.");
    expect([...host.querySelectorAll("a")].some(link => link.textContent === "Settings")).toBe(false);
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

it("updates the mounted first-run picker after turning OFF in General and returning", async () => {
  const { AnalyticsToggle } = await import("../Settings/AnalyticsToggle");
  const on = { consent: "on", queued: 0, has_device_id: true, has_user_id: false };
  const off = { consent: "off", queued: 0, has_device_id: false, has_user_id: false };
  let stored = on;
  localJson.mockImplementation(async (method: string, _path: string, body?: { consent: string }) => {
    if (method === "POST") { expect(body).toEqual({ consent: "off" }); stored = off; }
    return stored;
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let showSettings!: () => void;
  const open = () => showSettings();
  function Screen() {
    const [settings, setSettings] = useState(false);
    showSettings = () => setSettings(true);
    return <>
      <div hidden={settings}><FirstRunGeneralPicker providers={[]} /></div>
      {settings && <section aria-label="General"><AnalyticsToggle /><button onClick={() => setSettings(false)}>Back</button></section>}
    </>;
  }
  window.addEventListener("codeterm:open-panel", open);
  try {
    await act(async () => root.render(<Screen />));
    const picker = host.querySelector('[data-testid="first-run-general-picker"]')!;
    const link = [...picker.querySelectorAll("a")].find(element => element.textContent === "Settings")!;
    expect(link.parentElement?.textContent).toBe("Product analytics is on. Turn off in Settings.");
    await act(async () => link.click());
    const toggle = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Share product analytics"]')!;
    await act(async () => toggle.click());
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(picker.textContent).not.toContain("Product analytics is on.");
    await act(async () => [...host.querySelectorAll("button")].find(button => button.textContent === "Back")!.click());
    expect(host.querySelector('[data-testid="first-run-general-picker"]')).toBe(picker);
    expect(picker.textContent).not.toContain("Product analytics is on.");
  } finally {
    window.removeEventListener("codeterm:open-panel", open);
    await act(async () => root.unmount()); host.remove();
  }
});


it("keeps confirmed OFF after an unmounted toggle's delayed ON response resolves", async () => {
  const { AnalyticsToggle } = await import("../Settings/AnalyticsToggle");
  const on = { consent: "on", queued: 0, has_device_id: true, has_user_id: false };
  const off = { consent: "off", queued: 0, has_device_id: false, has_user_id: false };
  let stored = off;
  let finishOn!: (value: typeof on) => void;
  const deferredOn = new Promise<typeof on>(resolve => { finishOn = resolve; });
  const publications: Array<string | undefined> = [];
  const observe = (event: Event) => publications.push((event as CustomEvent<{ status: typeof on | null }>).detail.status?.consent);
  localJson.mockImplementation(async (method: string, _path: string, body?: { consent: string }) => {
    if (method !== "POST") return stored;
    if (body?.consent === "on") { stored = on; return deferredOn; }
    expect(body).toEqual({ consent: "off" }); stored = off; return off;
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const screen = (toggle: string | null) => <>
    <FirstRunGeneralPicker providers={[]} />
    {toggle !== null && <AnalyticsToggle key={toggle} />}
  </>;
  window.addEventListener("codeterm:analytics-consent-changed", observe);
  try {
    await act(async () => root.render(screen("A")));
    const picker = host.querySelector('[data-testid="first-run-general-picker"]')!;
    const toggle = () => host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Share product analytics"]')!;
    expect(toggle().getAttribute("aria-checked")).toBe("false");
    await act(async () => toggle().click());
    expect(localJson).toHaveBeenCalledWith("POST", "/api/analytics/consent", { consent: "on" });
    await act(async () => root.render(screen(null)));
    await act(async () => root.render(screen("B")));
    expect(toggle().getAttribute("aria-checked")).toBe("true");
    await act(async () => toggle().click());
    expect(toggle().getAttribute("aria-checked")).toBe("false");
    expect(publications[publications.length - 1]).toBe("off");
    const confirmed = [...publications];
    await act(async () => finishOn(on));
    expect(publications).toEqual(confirmed);
    expect(toggle().getAttribute("aria-checked")).toBe("false");
    expect(host.querySelector('[data-testid="first-run-general-picker"]')).toBe(picker);
    expect(picker.textContent).not.toContain("Product analytics is on.");
  } finally {
    window.removeEventListener("codeterm:analytics-consent-changed", observe);
    await act(async () => root.unmount()); host.remove();
  }
});
