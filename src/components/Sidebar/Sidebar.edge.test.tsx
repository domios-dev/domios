import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { Sidebar } from "./Sidebar";
import { Titlebar } from "../Titlebar/Titlebar";
import { useSidebarViewport } from "../../hooks/useSidebarViewport";
import type { ThemeConfig } from "../../lib/types";

vi.mock("../../store/multiplexer", () => ({
  useMultiplexer: () => ({}),
  useMuxSelector: () => ({
    workspaces: [], tabs: {}, teams: [], notifications: [], tabStatuses: {},
    activeTabIds: {}, active_workspace_id: null, closedTabs: [], orphanedTabs: [],
  }),
}));
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false }) }));
vi.mock("../../hooks/useTouchOptimizedUi", () => ({ useTouchOptimizedUi: () => false }));
vi.mock("../../lib/bridge", () => ({ invoke: vi.fn().mockResolvedValue([]), listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../Titlebar/AccountTray", () => ({ AccountTray: () => null }));
vi.mock("../Titlebar/ExtensionsTray", () => ({ ExtensionsTray: () => null }));
vi.mock("../Titlebar/SupporterActivityTray", () => ({ SupporterActivityTray: () => null }));
vi.mock("../Titlebar/KeepAwakeTray", () => ({ KeepAwakeTray: () => null }));
vi.mock("../Titlebar/SettingsMenu", () => ({ SettingsMenu: () => null }));
vi.mock("../WebPeerPicker/WebPeerPicker", () => ({ WebPeerPicker: () => null }));
vi.mock("../Notifications/NotificationToast", () => ({ NotificationToastContainer: () => null }));
vi.mock("./SpinalCord", () => ({ SpinalCordButton: () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Shell() {
  const sidebar = useSidebarViewport();
  return <>
    <Titlebar theme={{} as ThemeConfig} showSidebarToggle onToggleSidebar={sidebar.toggle} />
    {sidebar.mounted && <Sidebar onCollapse={sidebar.close} />}
  </>;
}

it("keeps edge clicks open, resizes on drag, and collapses through the existing toggle", async () => {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const pointer = async (target: EventTarget, type: string, clientX: number, pointerType = "mouse") => {
    await act(async () => target.dispatchEvent(new PointerEvent(type, {
      bubbles: true, pointerId: 1, button: 0, clientX, pointerType,
    })));
  };
  try {
    await act(async () => root.render(<Shell />));
    const edge = host.querySelector<HTMLButtonElement>('[role="separator"][aria-label="Resize sidebar"]')!;
    expect(edge).not.toBeNull();
    for (const pointerType of ["mouse", "touch"]) {
      for (let count = 0; count < 5; count++) {
        await pointer(edge, "pointerdown", 280, pointerType);
        await pointer(document, "pointerup", 280, pointerType);
        await act(async () => edge.click());
        expect(host.querySelector('aside[aria-label="Sidebar"]')?.getAttribute("data-collapsed")).toBe("false");
        expect(edge.getAttribute("aria-valuenow")).toBe("280");
      }
    }
    await act(async () => edge.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
    expect(host.querySelector('aside[aria-label="Sidebar"]')?.getAttribute("data-collapsed")).toBe("false");
    await pointer(edge, "pointerdown", 280);
    await pointer(document, "pointermove", 340);
    await pointer(document, "pointerup", 340);
    await act(async () => edge.click());
    expect(edge.getAttribute("aria-valuenow")).toBe("340");
    expect(host.querySelector('aside[aria-label="Sidebar"]')?.getAttribute("data-collapsed")).toBe("false");
    const toggle = host.querySelector<HTMLButtonElement>('button[aria-label="Open sidebar"]')!;
    expect(toggle).not.toBeNull();
    await act(async () => toggle.click());
    expect(host.querySelector('aside[aria-label="Sidebar"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});
