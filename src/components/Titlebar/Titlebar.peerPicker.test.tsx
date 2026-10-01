import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ThemeConfig, Workspace } from "../../lib/types";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import type { ExtendedState } from "../../store/multiplexer/tree";

const fixture = vi.hoisted(() => ({ state: null as ExtendedState | null }));

vi.mock("../../store/multiplexer", () => ({
  useMuxState: () => fixture.state,
  useMultiplexer: () => ({}),
}));
vi.mock("../../hooks/useAgentProviders", () => ({
  useAgentProviders: () => ({ providers: [], loading: false }),
}));
vi.mock("../../hooks/useTouchOptimizedUi", () => ({ useTouchOptimizedUi: () => false }));
vi.mock("../Canvas/PlansProvider", () => ({
  usePlanIndexOptional: () => ({ index: {} }),
  useSidebarPlansOptional: () => ({ plans: [] }),
}));
vi.mock("../Sidebar/SidebarWorkspaceTabs", () => ({ SidebarWorkspaceTabs: () => null }));
vi.mock("../Sidebar/WorkspaceCardGrid", () => ({ WorkspaceCardGrid: () => null }));
vi.mock("../Sidebar/OrchestrationCreate", () => ({ OrchestrationCreate: () => null }));
vi.mock("../Terminal/NewSessionPicker", () => ({ NewSessionPicker: () => null }));
vi.mock("./KeepAwakeTray", () => ({ KeepAwakeTray: () => null }));
vi.mock("./SettingsMenu", () => ({ SettingsMenu: () => null }));
vi.mock("../Notifications/NotificationToast", () => ({ NotificationToastContainer: () => null }));
vi.mock("../../lib/bridge", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/bridge")>(),
  invoke: vi.fn(async () => undefined),
  listen: vi.fn(async () => () => undefined),
}));
vi.mock("../../lib/webEvents", () => ({ resetWebEvents: vi.fn(), webListen: vi.fn() }));

import { Titlebar } from "./Titlebar";
import { Sidebar } from "../Sidebar/Sidebar";
import { clearRelayTarget, relayTarget, selfHost, setRelayTarget, webJson } from "../../lib/webApi";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const general: Workspace = {
  id: "general", name: "General", protected: true, tabs: [], active_tab_id: null, cwd: null,
};
const peers = [
  { instance_id: "broken", instance_name: "Broken", online: true, relay_protocol_version: RELAY_PROTOCOL_VERSION },
  { instance_id: "healthy", instance_name: "Healthy", online: true, relay_protocol_version: RELAY_PROTOCOL_VERSION },
];
const fetchMock = vi.fn<typeof fetch>();
let root: Root | null = null;
let host: HTMLDivElement;

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  vi.stubGlobal("innerWidth", 1024);
  Object.defineProperty(window, "innerWidth", { value: 1024, configurable: true });
  vi.stubGlobal("fetch", fetchMock);
  fetchMock.mockReset();
  fetchMock.mockImplementation(async (url) => {
    if (String(url) === `${selfHost()}/api/mesh/peers`) {
      return new Response(JSON.stringify({ local_instance_name: "Gateway", paired: peers }));
    }
    throw new Error(`Unexpected request: ${String(url)}`);
  });
  fixture.state = {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], toasts: [], tabStatuses: {}, activeTabIds: {}, generation: 0, pendingCloses: [],
  };
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
  clearRelayTarget();
  vi.unstubAllGlobals();
});

async function renderShell(sidebarMounted = true) {
  await act(async () => root!.render(
    <>
      <Titlebar theme={{} as ThemeConfig} showSidebarToggle={!sidebarMounted} onToggleSidebar={vi.fn()} />
      {sidebarMounted && <Sidebar />}
    </>,
  ));
}

function pickerButton() {
  const button = host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]');
  expect(button).not.toBeNull();
  expect(host.querySelectorAll('[data-testid="peer-picker"]')).toHaveLength(1);
  expect(button!.closest('[data-testid="titlebar"]')).not.toBeNull();
  expect(button!.closest('[data-testid="sidebar"]')).toBeNull();
  expect(button!.disabled).toBe(false);
  return button!;
}

function sidebarHandle() {
  return host.querySelector<HTMLButtonElement>('[aria-controls="codeterm-sidebar"]')!;
}

async function openHosts() {
  await act(async () => pickerButton().click());
  const menu = document.querySelector<HTMLDivElement>('[role="menu"]');
  expect(menu).not.toBeNull();
  return menu!;
}

describe("web host picker placement", () => {
  it("renders once in the titlebar with no workspaces or General section", async () => {
    await renderShell();
    expect(host.querySelector('[data-testid="general-section"]')).toBeNull();
    pickerButton();
  });

  it("stays mounted when the sidebar collapses", async () => {
    fixture.state!.workspaces = [general];
    await renderShell();
    expect(host.querySelector('[data-testid="general-section"]')).not.toBeNull();
    const trigger = pickerButton();
    await act(async () => sidebarHandle().click());
    expect(sidebarHandle().getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector('[data-testid="general-section"]')).toBeNull();
    expect(pickerButton()).toBe(trigger);
  });

  it("stays mounted when resizing the sidebar into compact mode", async () => {
    fixture.state!.workspaces = [general];
    await renderShell();
    const trigger = pickerButton();
    await act(async () => sidebarHandle().dispatchEvent(new MouseEvent("mousedown", { bubbles: true, clientX: 220 })));
    await act(async () => document.dispatchEvent(new MouseEvent("mousemove", { clientX: 80 })));
    await act(async () => document.dispatchEvent(new MouseEvent("mouseup")));
    expect(host.querySelector<HTMLElement>('[data-testid="sidebar"]')!.style.width).toBe("80px");
    expect(sidebarHandle().getAttribute("aria-expanded")).toBe("true");
    expect(host.querySelector('[data-testid="general-section"]')).toBeNull();
    expect(pickerButton()).toBe(trigger);
  });

  it("keeps the picker outside clipping and responsive hiding on a phone with the sidebar unmounted", async () => {
    vi.stubGlobal("innerWidth", 320);
    Object.defineProperty(window, "innerWidth", { value: 320, configurable: true });
    await renderShell(false);
    const trigger = pickerButton();
    expect(host.querySelector('[data-testid="sidebar"]')).toBeNull();
    const buttons = Array.from(host.querySelectorAll("button"));
    expect(buttons[0].dataset.testid).toBe("web-sidebar-open");
    expect(buttons[1]).toBe(trigger);
    for (let ancestor = trigger.parentElement; ancestor && ancestor !== host; ancestor = ancestor.parentElement) {
      expect(ancestor.className).not.toMatch(/\b(?:hidden|overflow-hidden)\b/);
    }
    expect(trigger.className).toContain("min-h-[44px]");
    expect(trigger.closest('[data-testid="titlebar"]')!.className).toContain("z-50");
  });
});

describe("web host picker recovery", () => {
  it.each(["error", "timeout", "pending"] as const)("lists gateway peers and switches while current-peer state is %s", async (failure) => {
    setRelayTarget("broken", "Broken");
    const fetchPeers = fetchMock.getMockImplementation()!;
    fetchMock.mockImplementation((url, init) => {
      if (String(url).endsWith("/api/mesh/relay-to")) {
        if (failure === "pending") {
          return new Promise<Response>((_resolve, reject) => {
            init?.signal?.addEventListener("abort", () => reject(new DOMException("Cancelled", "AbortError")), { once: true });
          });
        }
        return Promise.reject(new Error(failure));
      }
      return fetchPeers(url, init);
    });
    const stateLoad = webJson("GET", "/api/state").catch((error: unknown) => error);
    if (failure !== "pending") await stateLoad;
    await renderShell(false);
    const menu = await openHosts();
    expect(menu.querySelectorAll('[role="menuitem"]')).toHaveLength(3);
    expect(fetchMock.mock.calls.some(([url]) => String(url) === `${selfHost()}/api/mesh/peers`)).toBe(true);
    const healthy = menu.querySelector<HTMLButtonElement>('[data-peer-name="Healthy"]')!;
    expect(healthy.disabled).toBe(false);
    await act(async () => healthy.click());
    expect(relayTarget()?.id).toBe("healthy");
    expect(pickerButton().disabled).toBe(false);
    expect(document.activeElement).toBe(pickerButton());
    const gatewayMenu = await openHosts();
    await act(async () => gatewayMenu.querySelector<HTMLButtonElement>('[data-testid="peer-picker-self"]')!.click());
    expect(relayTarget()).toBeNull();
  });

  it("moves menu focus with arrows and returns focus to the named trigger on Escape", async () => {
    await renderShell(false);
    const trigger = pickerButton();
    expect(trigger.getAttribute("aria-label")).toContain("Gateway");
    expect(trigger.getAttribute("aria-haspopup")).toBe("menu");
    const menu = await openHosts();
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    const items = menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]');
    expect(document.activeElement).toBe(items[0]);
    await act(async () => items[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
    expect(document.activeElement).toBe(items[1]);
    await act(async () => items[1].dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
  });
});
