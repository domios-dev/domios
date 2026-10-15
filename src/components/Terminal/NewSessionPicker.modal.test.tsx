import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppConfig, Workspace } from "../../lib/types";

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
      addNotification: vi.fn(),
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
    },
    presets: { presets: [{ provider: "opencode", name: "demo", model: "live-model" }], loading: false, error: null },
  };
});
const workspace: Workspace = fixture.workspace;

vi.mock("../../store/multiplexer", () => ({
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
import { NewSessionPicker } from "./NewSessionPicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement;

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "single", providerId: "opencode" }));
  fixture.workspace.cwd_missing = false;
  fixture.actions.addNotification.mockClear();
  fixture.actions.setWorkspaceCwd.mockReset();
  fixture.actions.setWorkspaceCwd.mockResolvedValue({ warnings: [] });
  fixture.actions.spawnAgentTab.mockReset();
  fixture.actions.spawnAgentTab.mockResolvedValue(null);
  fixture.actions.createOrchestrationPair.mockReset();
  fixture.actions.createOrchestrationPair.mockResolvedValue({ orchestrator_tab_id: "lead-tab" });
  fixture.actions.selectTabByTab.mockClear();
  fixture.state.workspaces = [fixture.workspace];
  fixture.providers.providers = [{ id: "opencode", display_name: "OpenCode", available_models: [], reasoning_efforts: [] }] as never[];
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
});

async function renderPicker(props: { onClose?: () => void; fullAccess?: boolean; section?: "general" | "workspace" } = {}) {
  await act(async () => {
    root!.render(
      <NewSessionPicker
        workspaceId={workspace.id}
        section={props.section}
        workspaceCwd={workspace.cwd}
        agentConfig={props.fullAccess === undefined ? undefined : { full_access: props.fullAccess } as AppConfig["agent"]}
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
    .find((button) => button.getAttribute("aria-label") === name) ?? null;
}

async function choose(control: string, option: string) {
  const button = buttonNamed(control);
  expect(button).not.toBeNull();
  await act(async () => {
    button!.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true }));
  });
  const item = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"]'))
    .find((element) => element.textContent?.trim() === option);
  expect(item).toBeDefined();
  await act(async () => item!.click());
}

describe("NewSessionPicker modal", () => {
  it("keeps role controls labelled without a workspace control or explanatory rows", async () => {
    fixture.providers.providers = [{ id: "opencode", display_name: "OpenCode",
      available_models: [{ id: "model", display_name: "Model value" }], default_model: "model",
      reasoning_efforts: [{ id: "high", display_name: "High" }] }] as never[];
    await renderPicker({ fullAccess: false });
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
    expect(buttonNamed("Orchestrator model")).not.toBeNull();
    expect(buttonNamed("Planner model")).not.toBeNull();
    expect(buttonNamed("Orchestrator effort")).not.toBeNull();
    expect(buttonNamed("Planner effort")).not.toBeNull();
    expect(buttonNamed("Orchestrator model")?.textContent?.trim()).toBe("Model value");
    expect(buttonNamed("Planner effort")?.textContent?.trim()).toBe("High");
    expect(dialog().textContent).not.toContain("MODEL");
    expect(dialog().textContent).not.toContain("EFFORT");
  });

  it("restores a remembered Team as an agent when opened in General", async () => {
    localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "pair", orchestratorProviderId: "opencode", plannerProviderId: "opencode" }));
    await renderPicker({ fullAccess: false, section: "general" });
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
    await renderPicker({ fullAccess: false });
    expect(buttonNamed("Team")?.getAttribute("aria-pressed")).toBe("true");
    expect(buttonNamed("Orchestrator model")).not.toBeNull();
    expect(buttonNamed("Planner effort")).not.toBeNull();
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
    await renderPicker({ fullAccess: false });
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
    await renderPicker({ fullAccess: false });
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

  it("asks for agent access inline only until the user has decided", async () => {
    await renderPicker();
    expect(dialog().querySelector('input[type="checkbox"]')).not.toBeNull();

    await act(async () => root!.unmount());
    root = createRoot(host);
    await renderPicker({ fullAccess: false });
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

it("keeps a team request open and busy, then renders its backend failure inline", async () => {
  const onClose = vi.fn();
  let reject!: (error: unknown) => void;
  fixture.actions.createOrchestrationPair.mockImplementationOnce(() => new Promise((_, fail) => { reject = fail; }));
  await renderPicker({ fullAccess: false, onClose });
  await act(async () => buttonNamed("Team")!.click());
  const start = dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!;
  await act(async () => start.click());
  expect(fixture.actions.createOrchestrationPair).toHaveBeenCalledTimes(1);
  expect(start.disabled).toBe(true);
  expect(onClose).not.toHaveBeenCalled();
  expect(buttonNamed("Close new session dialog")!.disabled).toBe(true);
  const message = "opaque backend workspace/path/recovery sentence";
  await act(async () => reject(new TransportError(422, "/api/orchestration/new",
    JSON.stringify({ error: { code: "workspace_cwd_missing", message } }))));
  expect(dialog().querySelector('[role="alert"] details pre')?.textContent).toContain(message);
  expect(start.disabled).toBe(false);
  expect(onClose).not.toHaveBeenCalled();
  expect(fixture.actions.addNotification).not.toHaveBeenCalled();
});

it("renders a single-agent failure inline without a bell notification", async () => {
  const message = "opaque session refusal";
  fixture.actions.spawnAgentTab.mockRejectedValueOnce(new TransportError(422, "/api/agents/spawn",
    JSON.stringify({ error: { message } })));
  const onClose = vi.fn();
  await renderPicker({ fullAccess: false, onClose });
  await act(async () => dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!.click());
  expect(dialog().querySelector('[role="alert"] details pre')?.textContent).toContain(message);
  expect(onClose).not.toHaveBeenCalled();
  expect(fixture.actions.addNotification).not.toHaveBeenCalled();
});

it("offers folder relocation instead of session or team start for a missing workspace", async () => {
  fixture.workspace.cwd_missing = true;
  fixture.actions.setWorkspaceCwd.mockImplementationOnce(async () => {
    fixture.workspace.cwd_missing = false;
    return { warnings: [] };
  });
  await renderPicker({ fullAccess: false });
  expect(dialog().querySelector('[data-testid="picker-confirm"]')).toBeNull();
  expect(buttonNamed("Team")).toBeNull();
  const choose = Array.from(dialog().querySelectorAll("button")).find((button) => button.textContent === "Choose folder");
  expect(choose).toBeDefined();
  await act(async () => choose!.click());
  expect(fixture.actions.setWorkspaceCwd).toHaveBeenCalledExactlyOnceWith("workspace", "/relocated");
  await renderPicker({ fullAccess: false });
  expect(dialog().querySelector('[data-testid="picker-confirm"]')).not.toBeNull();
  expect(fixture.actions.spawnAgentTab).not.toHaveBeenCalled();
  expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
});
