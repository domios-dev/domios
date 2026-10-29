import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { Sidebar } from "./Sidebar";
import type { AppConfig } from "../../lib/types";
import type { NeighborList } from "../Neighbors/neighborModel";

const data = vi.hoisted(() => ({ friends: [] as NeighborList["friends"] }));
vi.mock("../../store/multiplexer", () => ({
  useMultiplexer: () => ({}),
  useMuxSelector: (select: (state: object) => unknown) => select({
    workspaces: [], tabs: {}, teams: [], notifications: [], tabStatuses: {},
    activeTabIds: {}, active_workspace_id: null, closedTabs: [], orphanedTabs: [],
  }),
}));
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false }) }));
vi.mock("../../hooks/useTouchOptimizedUi", () => ({ useTouchOptimizedUi: () => false }));
vi.mock("../../lib/bridge", () => ({ invoke: vi.fn().mockResolvedValue([]), listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../Settings/useAccount", () => ({ useAccount: () => ({ model: { kind: "signed_in", identity: { uid: "owner" } } }) }));
vi.mock("../Neighbors/useNeighbors", () => ({ useNeighbors: () => ({ list: { access: "paid", friends: data.friends } }) }));
vi.mock("../Neighbors/useConciergeStatus", () => ({ useConciergeStatus: () => ({}) }));
vi.mock("../Neighbors/neighborStatusStore", () => ({ useNeighborStatus: () => null, resetNeighborStatus: vi.fn() }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const config = { general_agent: { first_run_complete: true }, agent: { full_access: false }, mesh: { enabled: false } } as AppConfig;
afterEach(() => { localStorage.clear(); vi.unstubAllGlobals(); });

async function mount() {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Sidebar config={config} />));
  return { host, root, close: async () => { await act(async () => root.unmount()); host.remove(); } };
}

it("replaces Recently closed with one collapsed Neighbors section immediately before the sidebar views", async () => {
  data.friends = [{ uid: "ada", friendship_id: "pair", displayName: "Ada", photoURL: null, receiving: true, since: 1, inbound_per_hour: 10, outbound_per_hour: 10 }];
  const view = await mount();
  try {
    const sidebar = view.host.querySelector('aside[aria-label="Sidebar"]')!;
    expect(sidebar).not.toBeNull();
    expect(sidebar.textContent).not.toMatch(/recently closed/i);
    expect(sidebar.querySelectorAll('section[aria-label="Neighbors"]')).toHaveLength(1);
    const section = sidebar.querySelector('section[aria-label="Neighbors"]')!;
    expect(section.nextElementSibling).toBe(sidebar.querySelector('[role="navigation"][aria-label="Sidebar views"]'));
    const toggle = section.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
    expect(toggle.textContent).toBe("Neighbors");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(section.querySelector('ul[aria-label="Your Neighbors"]')).toBeNull();
    await act(async () => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const row = section.querySelector<HTMLButtonElement>('ul[aria-label="Your Neighbors"] button[aria-label="Open Ada"]')!;
    expect(row).not.toBeNull();
    const opened = vi.fn();
    window.addEventListener("codeterm:open-panel", opened);
    try {
      await act(async () => row.click());
      expect((opened.mock.calls[0][0] as CustomEvent).detail).toEqual({ panel: "neighbor:pair" });
    } finally { window.removeEventListener("codeterm:open-panel", opened); }
    await act(async () => toggle.click());
    expect(section.querySelector('ul[aria-label="Your Neighbors"]')).toBeNull();
  } finally { await view.close(); }
});

it("renders no Neighbors section or Recently closed section when there are no friends", async () => {
  data.friends = [];
  const view = await mount();
  try {
    expect(view.host.querySelector('section[aria-label="Neighbors"]')).toBeNull();
    expect(view.host.textContent).not.toMatch(/recently closed/i);
    expect(view.host.querySelector('[role="navigation"][aria-label="Sidebar views"]')).not.toBeNull();
  } finally { await view.close(); }
});
