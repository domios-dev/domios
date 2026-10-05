import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppConfig, Workspace } from "../../lib/types";

const workspace: Workspace = {
  id: "workspace", name: "Workspace", tabs: [], active_tab_id: null, cwd: "/workspace",
};

vi.mock("../../store/multiplexer", () => ({
  useMuxState: () => ({ workspaces: [workspace], active_workspace_id: workspace.id }),
  useMultiplexer: () => ({
    spawnAgentTab: vi.fn(),
    createOrchestrationPair: vi.fn(),
    selectTabByTab: vi.fn(),
    addNotification: vi.fn(),
    createWorkspace: vi.fn(),
  }),
}));
vi.mock("../../hooks/useAgentProviders", () => ({
  useAgentProviders: () => ({ providers: [], loading: false }),
}));
vi.mock("../../lib/providerInstallStatus", () => ({
  probeProviderInstallMap: vi.fn(async () => ({})),
}));
vi.mock("../../lib/bridge", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/bridge")>(),
  invoke: vi.fn(async () => undefined),
  listen: vi.fn(async () => () => undefined),
}));

import { NewSessionPicker } from "./NewSessionPicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement;

beforeEach(() => {
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
});

async function renderPicker(props: { onClose?: () => void; fullAccess?: boolean } = {}) {
  await act(async () => {
    root!.render(
      <NewSessionPicker
        workspaceId={workspace.id}
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

describe("NewSessionPicker modal", () => {
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
});
