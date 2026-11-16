import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Workspace } from "../../lib/types";

const fixture = vi.hoisted(() => {
  const workspace = {
    id: "workspace", name: "Workspace", tabs: [] as string[], active_tab_id: null, cwd: "/workspace", cwd_missing: false,
  };
  return {
    workspace,
    state: { workspaces: [workspace], active_workspace_id: workspace.id, focused_tab_id: "manager",
      tabs: { manager: { id: "manager", role: "Manager", agent_type: "opencode", session_binding_nonce: "manager-nonce" } } },
    actions: {
      spawnAgentTab: vi.fn(async () => null),
      createOrchestrationPair: vi.fn(async () => ({ orchestrator_tab_id: "lead-tab" })),
      selectTabByTab: vi.fn(async () => undefined),
      upsertToast: vi.fn(),
      setWorkspaceCwd: vi.fn(async () => ({ warnings: [] })),
    },
    folderDialog: vi.fn(async () => "/relocated"),
    providers: {
      providers: [
        {
          id: "opencode",
          display_name: "OpenCode",
          available_models: [],
          default_model: undefined,
          reasoning_efforts: [],
        },
      ] as never[],
      loading: false,
      refetch: vi.fn(),
    },
    presets: { presets: [{ provider: "opencode", name: "demo", model: "live-model" }], loading: false, error: null },
  };
});
const workspace: Workspace = fixture.workspace;

vi.mock("../../store/multiplexer/hooks", () => ({
  useMuxState: () => fixture.state,
  useMultiplexer: () => fixture.actions,
}));
vi.mock("../../hooks/useAgentProviders", () => ({
  useAgentProviders: () => fixture.providers,
}));
vi.mock("../../hooks/useModelPresets", () => ({ useModelPresets: () => fixture.presets }));
vi.mock("../../lib/providerInstallStatus", () => ({
  probeProviderInstallMap: vi.fn(async () => ({})),
}));
vi.mock("../../lib/bridge", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/bridge")>(),
  invoke: vi.fn(async () => undefined),
  listen: vi.fn(async () => () => undefined),
}));

vi.mock("../../lib/nativeBridge", () => ({
  nativeBridge: { dialogs: {} }, nativeFolderDialog: fixture.folderDialog,
}));

import { TransportError } from "../../lib/transportClient";
import { invoke } from "../../lib/bridge";
import { NewSessionPicker } from "./NewSessionPicker";
import { cloudExecution } from "./cloudExecution";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement;

beforeEach(() => {
  vi.mocked(invoke).mockClear();
  localStorage.clear();
  localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "single", providerId: "opencode" }));
  fixture.workspace.cwd_missing = false;
  fixture.actions.upsertToast.mockClear();
  fixture.actions.setWorkspaceCwd.mockReset();
  fixture.actions.setWorkspaceCwd.mockResolvedValue({ warnings: [] });
  fixture.actions.spawnAgentTab.mockReset();
  fixture.actions.spawnAgentTab.mockResolvedValue(null);
  fixture.actions.createOrchestrationPair.mockReset();
  fixture.actions.createOrchestrationPair.mockResolvedValue({ orchestrator_tab_id: "lead-tab" });
  fixture.actions.selectTabByTab.mockClear();
  fixture.state.workspaces = [fixture.workspace];
  fixture.providers.providers = [{ id: "opencode", display_name: "OpenCode", available_models: [], reasoning_efforts: [] }] as never[];
  fixture.providers.refetch.mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  cloudExecution.enabled = true;
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
});

async function renderPicker(props: { onClose?: () => void; section?: "general" | "workspace" } = {}) {
  await act(async () => {
    root!.render(
      <NewSessionPicker
        workspaceId={workspace.id}
        section={props.section}
        workspaceCwd={workspace.cwd}
        onClose={props.onClose ?? (() => undefined)}
      />,
    );
  });
}

function dialog(): HTMLElement {
  const element = document.body.querySelector<HTMLElement>('[role="dialog"]');
  expect(element).not.toBeNull();
  return element!;
}

function buttonNamed(name: string): HTMLButtonElement | null {
  return Array.from(dialog().querySelectorAll<HTMLButtonElement>("button[aria-label]"))
    .find((button) => {
      const label = button.getAttribute("aria-label") ?? "";
      return label === name || label.startsWith(`${name}:`);
    }) ?? null;
}

async function choose(control: string, option: string) {
  const button = buttonNamed(control);
  expect(button).not.toBeNull();
  await act(async () => {
    button!.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true }));
  });
  const item = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"]'))
    .find((element) => element.textContent?.trim() === option);
  expect(item).toBeDefined();
  await act(async () => item!.click());
}

describe("NewSessionPicker modal", () => {
  it("keeps role controls labelled without a workspace control or explanatory rows", async () => {
    fixture.providers.providers = [{ id: "opencode", display_name: "OpenCode",
      available_models: [{ id: "model", display_name: "Model value" }], default_model: "model",
      reasoning_efforts: [{ id: "high", display_name: "High" }] }] as never[];
    await renderPicker();
    expect(buttonNamed("Workspace")).toBeNull();
    expect(buttonNamed("Agent provider")).not.toBeNull();
    expect(buttonNamed("Agent model")).not.toBeNull();
    const content = dialog().textContent ?? "";
    expect(content).not.toContain("Choose a single agent or an Orchestrator + Planner pair.");
    expect(content).not.toContain("Your last choice is remembered.");
    expect(content).not.toContain("Creates in:");
    expect(content).not.toContain("will open");
    expect(dialog().querySelector('[data-testid="workspace-help"]')).toBeNull();
    await act(async () => buttonNamed("Team")!.click());
    // Team rows are compact: one model · effort pill opens the unified model picker; there is no separate effort select.
    expect(buttonNamed("Orchestrator model")).not.toBeNull();
    expect(buttonNamed("Planner model")).not.toBeNull();
    expect(buttonNamed("Orchestrator effort")).toBeNull();
    expect(buttonNamed("Orchestrator model")?.textContent?.trim()).toBe("Model value · High");
    expect(buttonNamed("Planner model")?.getAttribute("aria-haspopup")).toBe("dialog");
    expect(dialog().textContent).not.toContain("MODEL");
    expect(dialog().textContent).not.toContain("EFFORT");
  });

  it("restores a remembered Team as an agent when opened in General", async () => {
    localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "pair", orchestratorProviderId: "opencode", plannerProviderId: "opencode" }));
    await renderPicker({ section: "general" });
    expect(buttonNamed("Team")).toBeNull();
    expect(buttonNamed("Orchestrator provider")).toBeNull();
    expect(buttonNamed("Planner provider")).toBeNull();
    expect(buttonNamed("Agent provider")).not.toBeNull();
  });

  it("switches modes and starts a team in the opening workspace with both role settings", async () => {
    fixture.state.workspaces = [fixture.workspace, { ...fixture.workspace, id: "other", name: "Other", cwd: "/other" }];
    fixture.providers.providers = [
      { id: "opencode", display_name: "OpenCode", available_models: [{ id: "lead", display_name: "Lead" }],
        default_model: "lead", reasoning_efforts: [{ id: "high", display_name: "High" }] },
      { id: "peer", display_name: "Peer", available_models: [{ id: "plan", display_name: "Plan" }],
        default_model: "plan", reasoning_efforts: [{ id: "low", display_name: "Low" }] },
    ] as never[];
    localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "pair",
      orchestratorProviderId: "opencode", orchestratorModelId: "lead", orchestratorEffortId: "high",
      plannerProviderId: "peer", plannerModelId: "plan", plannerEffortId: "low" }));
    await renderPicker();
    expect(buttonNamed("Team")?.getAttribute("aria-pressed")).toBe("true");
    expect(buttonNamed("Orchestrator model")).not.toBeNull();
    expect(buttonNamed("Planner model")?.textContent?.trim()).toBe("Plan · Low");
    await act(async () => buttonNamed("Single agent")!.click());
    expect(buttonNamed("Single agent")?.getAttribute("aria-pressed")).toBe("true");
    expect(buttonNamed("Agent model")?.getAttribute("data-value")).toBe("lead");
    await act(async () => buttonNamed("Team")!.click());
    await choose("Planner provider", "Peer");
    await act(async () => dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!.click());
    expect(fixture.actions.createOrchestrationPair).toHaveBeenCalledExactlyOnceWith({
      orchestratorAgent: "opencode", orchestratorModel: "lead", orchestratorReasoningEffort: "high",
      plannerAgent: "peer", plannerModel: "plan", plannerReasoningEffort: "low", workspaceId: "workspace",
    });
    expect(fixture.actions.selectTabByTab).toHaveBeenCalledExactlyOnceWith("workspace", "lead-tab");
    expect(fixture.actions.spawnAgentTab).not.toHaveBeenCalled();
  });

  it("starts a single agent in the opening workspace with the chosen provider, model and effort", async () => {
    fixture.state.workspaces = [fixture.workspace, { ...fixture.workspace, id: "other", name: "Other", cwd: "/other" }];
    fixture.providers.providers = [{ id: "opencode", display_name: "OpenCode",
      available_models: [{ id: "first", display_name: "First" }, { id: "chosen", display_name: "Chosen" }],
      default_model: "first", reasoning_efforts: [{ id: "low", display_name: "Low" }, { id: "high", display_name: "High" }] }] as never[];
    await renderPicker();
    await choose("Agent model", "Chosen");
    await choose("Agent effort", "High");
    await act(async () => dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!.click());
    expect(fixture.actions.spawnAgentTab).toHaveBeenCalledExactlyOnceWith("workspace", "opencode", "/workspace", "chosen", "high", undefined);
    expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
  });

  it("spawns the selected owned preset with its model once", async () => {
    fixture.providers.providers = [{ id: "opencode", display_name: "OpenCode",
      available_models: [{ id: "live-model", display_name: "Live model" }], default_model: "live-model", reasoning_efforts: [] }] as never[];
    localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "single", providerId: "opencode", modelId: "live-model" }));
    await renderPicker();
    const preset = dialog().querySelector<HTMLSelectElement>('select[aria-label="Preset"]');
    expect(preset).not.toBeNull();
    await act(async () => { preset!.value = "demo"; preset!.dispatchEvent(new Event("change", { bubbles: true })); });
    const start = Array.from(dialog().querySelectorAll("button")).find((button) => button.textContent === "Start session");
    expect(start).toBeDefined();
    await act(async () => start!.click());
    expect(fixture.actions.spawnAgentTab).toHaveBeenCalledExactlyOnceWith("workspace", "opencode", "/workspace", "live-model", undefined, "demo");
  });

  it("renders a labelled modal dialog portaled outside the opening surface", async () => {
    await renderPicker();

    const modal = dialog();
    expect(modal.getAttribute("aria-modal")).toBe("true");
    expect(host.contains(modal)).toBe(false);
    const titleId = modal.getAttribute("aria-labelledby");
    expect(titleId && document.getElementById(titleId)?.textContent?.trim()).toBeTruthy();
  });

  it("closes from its named close control and from Escape", async () => {
    const onClose = vi.fn();
    await renderPicker({ onClose });

    const close = dialog().querySelector<HTMLButtonElement>('button[aria-label="Close new session dialog"]');
    expect(close).not.toBeNull();
    await act(async () => close!.click());
    expect(onClose).toHaveBeenCalledTimes(1);

    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("starts without an agent access question", async () => {
    await renderPicker();
    expect(dialog().querySelector('input[type="checkbox"]')).toBeNull();

    expect(document.body.querySelectorAll('[role="dialog"]')).toHaveLength(1);
  });

  it("renders model changes from the shared provider catalogue", async () => {
    await renderPicker();
    expect(buttonNamed("Agent model")).not.toBeNull();

    fixture.providers.providers = [{
      id: "opencode",
      display_name: "OpenCode",
      catalogue_version: 1,
      available_models: [{ id: "live-model", display_name: "Live model" }],
      default_model: "live-model",
      reasoning_efforts: [],
    }] as never[];
    await renderPicker();

    expect(buttonNamed("Agent model")?.getAttribute("data-value")).toBe("live-model");
  });
});

it("dismisses a team request before it resolves and preserves its eventual error", async () => {
  const onClose = vi.fn();
  let reject!: (error: unknown) => void;
  fixture.actions.createOrchestrationPair.mockImplementationOnce(() => new Promise((_, fail) => { reject = fail; }));
  await renderPicker({ onClose });
  await act(async () => buttonNamed("Team")!.click());
  const start = dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!;
  await act(async () => start.click());
  expect(fixture.actions.createOrchestrationPair).toHaveBeenCalledTimes(1);
  expect(start.disabled).toBe(true);
  await act(async () => document.querySelector('[data-testid="new-session-picker"]')!.dispatchEvent(new Event("animationend", { bubbles: true })));
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(buttonNamed("Close new session dialog")!.disabled).toBe(true);
  const message = "opaque backend workspace/path/recovery sentence";
  await act(async () => reject(new TransportError(422, "/api/orchestration/new",
    JSON.stringify({ error: { code: "workspace_cwd_missing", message } }))));
  expect(fixture.actions.upsertToast).toHaveBeenCalledTimes(1);
  expect(fixture.actions.upsertToast.mock.calls[0][0].body).toContain(message);
  expect(start.disabled).toBe(false);
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("preserves a single-agent failure after dismissal", async () => {
  const message = "opaque session refusal";
  fixture.actions.spawnAgentTab.mockRejectedValueOnce(new TransportError(422, "/api/agents/spawn",
    JSON.stringify({ error: { message } })));
  const onClose = vi.fn();
  await renderPicker({ onClose });
  await act(async () => dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!.click());
  expect(fixture.actions.upsertToast).toHaveBeenCalledTimes(1);
  expect(fixture.actions.upsertToast.mock.calls[0][0].body).toContain(message);
});

it("offers folder relocation instead of session or team start for a missing workspace", async () => {
  fixture.workspace.cwd_missing = true;
  fixture.actions.setWorkspaceCwd.mockImplementationOnce(async () => {
    fixture.workspace.cwd_missing = false;
    return { warnings: [] };
  });
  await renderPicker();
  expect(dialog().querySelector('[data-testid="picker-confirm"]')).toBeNull();
  expect(buttonNamed("Team")).toBeNull();
  const choose = Array.from(dialog().querySelectorAll("button")).find((button) => button.textContent === "Choose folder");
  expect(choose).toBeDefined();
  await act(async () => choose!.click());
  expect(fixture.actions.setWorkspaceCwd).toHaveBeenCalledExactlyOnceWith("workspace", "/relocated");
  await renderPicker();
  expect(dialog().querySelector('[data-testid="picker-confirm"]')).not.toBeNull();
  expect(fixture.actions.spawnAgentTab).not.toHaveBeenCalled();
  expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
});

it("hides Cloud execution while it is disabled, without refreshing remote providers", async () => {
  cloudExecution.enabled = false;
  Object.assign(fixture.providers.providers.find((provider: {id: string}) => provider.id === "opencode")!, {remote_task: {
    backend: "fixture", mode: "cloud", displayName: "Cloud", availability: {status: "available"},
    environments: [{id: "env-fixture", label: "Fixture environment", accountRef: "account-fixture"}], followUp: false, cancel: false, applyDiff: false,
  }} as never);
  await renderPicker();
  expect(dialog().querySelector('input[name="execution-location"]')).toBeNull();
  expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("refresh_provider_models", {providerId: "opencode"});
});

it("offers Remote only through declared capability and spawns the selected environment without local model controls", async () => {
  expect(cloudExecution.enabled).toBe(true);
  Object.assign(fixture.providers.providers.find((provider: {id: string}) => provider.id === "opencode")!, {remote_task: {
    backend: "fixture", mode: "cloud", displayName: "Cloud", availability: {status: "available"},
    environments: [{id: "env-fixture", label: "Fixture environment", accountRef: "account-fixture"}], followUp: false, cancel: false, applyDiff: false,
  }} as never);
  await renderPicker();
  expect(vi.mocked(invoke)).toHaveBeenCalledWith("refresh_provider_models", {providerId: "opencode"});
  expect(fixture.providers.refetch).toHaveBeenCalledExactlyOnceWith();
  await act(async () => dialog().querySelector<HTMLInputElement>('input[value="cloud"]')!.click());
  const picker = dialog().querySelector<HTMLSelectElement>('select[aria-label="Cloud environment"]');
  expect(picker).not.toBeNull();
  await act(async () => {picker!.value = "env-fixture"; picker!.dispatchEvent(new window.Event("change", {bubbles: true}));});
  expect(buttonNamed("Agent model")).toBeNull();
  await act(async () => dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!.click());
  expect(fixture.actions.spawnAgentTab).toHaveBeenCalledExactlyOnceWith("workspace", "opencode", "/workspace", undefined, undefined, "env-fixture", undefined, "cloud");
});

it("keeps an empty remote environment list actionable and refuses spawn", async () => {
  cloudExecution.enabled = true;
  Object.assign(fixture.providers.providers.find((provider: {id: string}) => provider.id === "opencode")!, {remote_task: {
    backend: "fixture", mode: "cloud", displayName: "Cloud", availability: {status: "available"}, environments: [], followUp: false, cancel: false, applyDiff: false,
  }} as never);
  await renderPicker();
  await act(async () => dialog().querySelector<HTMLInputElement>('input[value="cloud"]')!.click());
  expect(Array.from(dialog().querySelectorAll("button")).find(button => button.textContent === "Refresh harnesses")).toBeDefined();
  expect(dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')?.disabled).toBe(true);
  expect(fixture.actions.spawnAgentTab).not.toHaveBeenCalled();
});

it("shows Cloud only under Codex and explains its unavailable environment state", async () => {
  cloudExecution.enabled = true;
  const reason = "Sign in to Codex to discover Cloud environments.";
  fixture.providers.providers = [{ id: "codex", display_name: "Codex", available_models: [], reasoning_efforts: [],
    remote_task: { backend: "chatgpt-cloud-tasks", mode: "cloud", displayName: "Cloud",
      availability: { status: "unavailable", reason }, environments: [], followUp: false, cancel: false, applyDiff: false } }] as never[];
  localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "single", providerId: "codex" }));

  await renderPicker();

  const providerButton = buttonNamed("Agent provider");
  expect(providerButton?.textContent).toContain("Codex");
  await act(async () => providerButton!.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true })));
  const providerOptions = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"]'))
    .map((item) => item.textContent?.trim());
  expect(providerOptions).toEqual(["Codex"]);
  await act(async () => document.body.querySelector<HTMLElement>('[role="menuitem"], [role="menuitemradio"]')!.click());
  expect(dialog().querySelectorAll('input[name="execution-location"][value="cloud"]')).toHaveLength(1);
  expect(dialog().textContent).not.toContain("Codex Cloud");
  await act(async () => dialog().querySelector<HTMLInputElement>('input[name="execution-location"][value="cloud"]')!.click());
  expect(dialog().textContent).toContain(reason);
  expect(dialog().textContent).toContain("No remote environments are available");
  expect(dialog().querySelector('select[aria-label="Cloud environment"]')).toBeNull();
  expect(dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')?.disabled).toBe(true);
  expect(fixture.actions.spawnAgentTab).not.toHaveBeenCalled();
});

it("creates a Team with independent ready profiles, models and efforts through the pair API", async () => {
  fixture.providers.providers = [
    {id: "opaque-runtime", display_name: "Runtime", transport: "chat_backend", available_models: [],
      agent_runtime: {launchProfiles: {alpha: {binary: "alpha", args: []}, beta: {binary: "beta", args: []}, missing: {binary: "missing", args: []}}}},
  ] as never[];
  const configOptions = [
    {id: "model", name: "Model", category: "model", currentValue: "small", options: [{value: "small", name: "Small"}, {value: "large", name: "Large"}]},
    {id: "effort", name: "Effort", category: "thought_level", currentValue: "low", options: [{value: "low", name: "Low"}, {value: "high", name: "High"}]},
  ];
  vi.mocked(invoke).mockImplementation(async command => command === "provider_launch_profiles" ? [
    {id: "alpha", install: "installed", auth: "unknown", canInstall: false, ownerConfigured: false, configOptions},
    {id: "beta", install: "detected", auth: "unknown", canInstall: false, ownerConfigured: false, configOptions},
    {id: "missing", install: "not_installed", auth: "unknown", canInstall: false, ownerConfigured: false},
  ] : undefined);
  await renderPicker();
  await act(async () => buttonNamed("Team")!.click());
  const start = () => dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!;
  const pick = async (scope: ParentNode, testId: string, option: string) => {
    const trigger = scope.querySelector<HTMLButtonElement>(`[data-testid="${testId}"]`)!;
    expect(trigger).not.toBeNull();
    await act(async () => trigger.dispatchEvent(new window.Event("pointerdown", {bubbles: true, cancelable: true})));
    await act(async () => document.body.querySelector<HTMLElement>(`[data-testid="${testId}-option-${option}"]`)!.click());
  };
  const orchestrator = dialog().querySelector('[data-testid="orchestration-role-orchestrator"]')!;
  const planner = dialog().querySelector('[data-testid="orchestration-role-planner"]')!;
  expect(start().disabled).toBe(true);
  expect(buttonNamed("Orchestrator provider")?.textContent).toContain("Runtime");
  expect(buttonNamed("Planner provider")?.textContent).toContain("Runtime");
  await pick(orchestrator, "picker-profile-orchestrator", "alpha");
  expect(start().disabled).toBe(true);
  await pick(planner, "picker-profile-planner", "missing");
  expect(start().disabled).toBe(true);
  await act(async () => start().click());
  expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
  await pick(planner, "picker-profile-planner", "beta");
  expect(start().disabled).toBe(false);
  await pick(orchestrator, "picker-profile-model", "large");
  await pick(orchestrator, "picker-profile-effort", "high");
  await pick(planner, "picker-profile-model", "small");
  await pick(planner, "picker-profile-effort", "low");
  await pick(orchestrator, "picker-profile-orchestrator", "beta");
  expect(orchestrator.querySelector('[data-testid="picker-profile-model"]')?.textContent).toContain("Default");
  expect(planner.querySelector('[data-testid="picker-profile-model"]')?.textContent).toContain("Small");
  await pick(orchestrator, "picker-profile-orchestrator", "alpha");
  await pick(orchestrator, "picker-profile-model", "large");
  await pick(orchestrator, "picker-profile-effort", "high");
  await act(async () => start().click());
  expect(fixture.actions.createOrchestrationPair).toHaveBeenCalledExactlyOnceWith({
    orchestratorAgent: "opaque-runtime", orchestratorLaunchProfile: "alpha", orchestratorModel: "large", orchestratorReasoningEffort: "high",
    plannerAgent: "opaque-runtime", plannerLaunchProfile: "beta", plannerModel: "small", plannerReasoningEffort: "low", workspaceId: "workspace",
  });
});

describe("NewSessionPicker launch profiles", () => {
  it("offers one ACP provider with a labelled nested choice and passes the chosen profile once", async () => {
    fixture.providers.providers = [
      {id:"opencode",display_name:"OpenCode",available_models:[],reasoning_efforts:[]},
      {id:"opaque-acp",display_name:"ACP",transport:"chat_backend",available_models:[],agent_runtime:{launchProfiles:{
        "deepseek-harness":{binary:"npx",args:["@deepseek-ai/dsh@0.2.1-rc.5","--profile","acp"]},
        "gemini-cli":{binary:"gemini",args:["--acp"]},
      }}},
    ] as never[];
    vi.mocked(invoke).mockImplementation(async command => command === "provider_launch_profiles" ? [
      {id:"deepseek-harness",install:"installed",auth:"unknown",canInstall:true,ownerConfigured:false},
      {id:"gemini-cli",install:"detected",auth:"unknown",canInstall:false,ownerConfigured:false},
    ] : undefined);
    await renderPicker();
    await choose("Agent provider","ACP");
    // The profile is a pill in the model's place in the Agent row, listing each profile with its state word.
    const pill = dialog().querySelector<HTMLButtonElement>('[data-testid="picker-profile-single"]');
    expect(pill?.getAttribute("aria-label")).toBe("Agent profile: Choose a profile");
    expect(dialog().querySelector('[role="radiogroup"]')).toBeNull();
    await act(async () => { pill!.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true })); });
    const options = [...document.body.querySelectorAll<HTMLElement>('[data-testid^="picker-profile-single-option-"]')];
    expect(options.map(option => option.textContent?.trim())).toEqual(["deepseek-harnessInstalled", "gemini-cliInstalled"]);
    expect(fixture.actions.spawnAgentTab).not.toHaveBeenCalled();
    await act(async () => options[1].click());
    expect(dialog().querySelector('[data-testid="picker-profile-single"]')?.getAttribute("aria-label")).toBe("Agent profile: gemini-cli");
    const start = dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]');
    expect(start).not.toBeNull();
    await act(async () => start!.click());
    expect(fixture.actions.spawnAgentTab).toHaveBeenCalledTimes(1);
    expect(fixture.actions.spawnAgentTab).toHaveBeenCalledWith("workspace","opaque-acp","/workspace",undefined,undefined,undefined,undefined,undefined,"gemini-cli");
  });

  it("after a ready ACP profile, offers the model and effort it last declared (none when it declares none), defaulting to the agent's own, and spawns with the choice", async () => {
    fixture.providers.providers = [
      {id:"opaque-acp",display_name:"ACP",transport:"chat_backend",available_models:[],agent_runtime:{launchProfiles:{
        "deepseek-harness":{binary:"npx",args:[]},
        "gemini-cli":{binary:"gemini",args:["--acp"]},
      }}},
    ] as never[];
    vi.mocked(invoke).mockImplementation(async command => command === "provider_launch_profiles" ? [
      {id:"deepseek-harness",install:"installed",auth:"unknown",canInstall:true,ownerConfigured:false,configOptions:[
        {id:"model",name:"Model",category:"model",currentValue:"flash",options:[{value:"flash",name:"deepseek-v4-flash"},{value:"pro",name:"DeepSeek-V4-Pro"}]},
        {id:"reasoning_effort",name:"Reasoning effort",category:"thought_level",currentValue:"high",options:[{value:"low",name:"Low"},{value:"high",name:"High"}]},
      ]},
      {id:"gemini-cli",install:"detected",auth:"unknown",canInstall:false,ownerConfigured:false,configOptions:[]},
    ] : undefined);
    await renderPicker();
    const pick = async (testId: string, option: string) => {
      const trigger = dialog().querySelector<HTMLButtonElement>(`[data-testid="${testId}"]`)!;
      await act(async () => { trigger.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true })); });
      await act(async () => document.body.querySelector<HTMLElement>(`[data-testid="${testId}-option-${option}"]`)!.click());
    };
    await pick("picker-profile-single", "gemini-cli");
    const model = () => dialog().querySelector<HTMLButtonElement>('[data-testid="picker-profile-model"]');
    // A profile that declares no model or effort offers no pills; the agent keeps its own default.
    expect(dialog().querySelector('[data-testid="launch-profile-model-row"]')).toBeNull();

    await pick("picker-profile-single", "deepseek-harness");
    expect(model()?.disabled).toBe(false);
    expect(model()?.textContent).toContain("Default");
    await pick("picker-profile-model", "pro");
    await pick("picker-profile-effort", "low");
    await act(async () => dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!.click());
    expect(fixture.actions.spawnAgentTab).toHaveBeenCalledWith("workspace","opaque-acp","/workspace","pro","low",undefined,undefined,undefined,"deepseek-harness");
  });
});
