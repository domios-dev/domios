import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { MeshPeersResponse, ProviderDescriptor, Tab } from "../../lib/types";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import { resetRemoteChatStatus, updateRemoteChatPeers } from "../../lib/remoteChatStatus";
import { rememberMachineIdentities } from "../../lib/useMeshMachineIdentities";
import {
  MultiplexerActionsContext,
  MuxStateStoreContext,
  type MultiplexerActionsContextType,
} from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";

vi.mock("../Sidebar/ProviderSwitchDialog", () => ({
  useProviderSwitchDialog: () => ({ openProviderSwitch: vi.fn(), providerSwitchDialog: null }),
}));
vi.mock("./useTelegramThreadMenu", () => ({
  useTelegramThreadMenu: () => ({ state: null, refresh: vi.fn() }),
}));
vi.mock("./TabActionsMenu", () => ({ TabActionsMenu: () => null }));
vi.mock("./ComposerContextMeter", () => ({ ComposerContextMeter: () => null }));
vi.mock("./DocumentsPanel", () => ({ DocumentsButton: () => null }));
vi.mock("./ChangesPanel", () => ({ ChangesButton: () => null }));
vi.mock("../Extensions/PluginBubbles", () => ({ PluginBubbles: () => null }));

import { TabTopBar } from "./TabTopBar";
import { Segmented } from "../shared/Segmented";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PRIMITIVES_CSS = readFileSync(resolve(process.cwd(), "src/styles/primitives.css"), "utf8");
const TAB_TOP_BAR_CSS = readFileSync(resolve(process.cwd(), "src/components/Terminal/tabTopBar.css"), "utf8");

function readCustomProp(el: HTMLElement, name: string): string {
  let node: HTMLElement | null = el;
  while (node) {
    const inline = node.style.getPropertyValue(name).trim();
    if (inline) return inline;
    const computed = getComputedStyle(node).getPropertyValue(name).trim();
    if (computed) return computed;
    node = node.parentElement;
  }
  return "";
}

function expandVars(expr: string, el: HTMLElement, depth = 0): string {
  if (depth > 8 || !expr.includes("var(")) return expr;
  let out = "";
  let i = 0;
  while (i < expr.length) {
    if (expr.startsWith("var(", i)) {
      let open = 1;
      let j = i + 4;
      let comma = -1;
      while (j < expr.length && open > 0) {
        if (expr[j] === "(") open += 1;
        else if (expr[j] === ")") open -= 1;
        else if (expr[j] === "," && open === 1 && comma === -1) comma = j;
        j += 1;
      }
      const name = expr.slice(i + 4, comma === -1 ? j - 1 : comma).trim();
      const fallback = comma === -1 ? "" : expr.slice(comma + 1, j - 1).trim();
      const resolved = readCustomProp(el, name) || fallback;
      out += `(${expandVars(resolved, el, depth + 1)})`;
      i = j;
    } else {
      out += expr[i];
      i += 1;
    }
  }
  return out;
}

function resolvePxLength(raw: string, el: HTMLElement): number | null {
  const value = raw.trim();
  if (!value || value === "auto" || value === "none" || value === "normal") return null;
  const expanded = expandVars(value, el)
    .replaceAll("calc(", "(")
    .replace(/(\d+(?:\.\d+)?)px\b/g, "$1");
  if (!/^[\d.+\-*/()\s]+$/.test(expanded)) return null;
  const result = Function(`"use strict"; return (${expanded});`)() as unknown;
  return typeof result === "number" && Number.isFinite(result) ? result : null;
}

function makeState(tab: Tab): ExtendedState {
  return {
    workspaces: [{ id: "ws-1", name: "WS", root_path: "/tmp", tabs: [tab.id], split_layouts: {}, active_tab_id: tab.id }],
    tabs: { [tab.id]: tab },
    teams: [],
    active_workspace_id: "ws-1",
    active_tab_id: tab.id,
    seq: 1,
    loading: false,
    connected: true,
    error: null,
  } as unknown as ExtendedState;
}

it.each([
  {
    name: "local tab header",
    tab: { id: "tab-local", title: "Ohm", agent_type: "claude", role: "Agent", cwd: "/tmp" } as unknown as Tab,
  },
  {
    name: "remote machine tab header",
    tab: {
      id: "mesh:eight/tab-verdi",
      title: "Verdi",
      agent_type: "claude",
      role: "Orchestrator",
      remote_peer_id: "eight",
      remote_peer_name: "eight",
      cwd: "/tmp",
    } as unknown as Tab,
  },
])("sizes both Chat and Terminal segments to 22px cells inside a 48x26 switch in $name", async ({ tab }) => {
  setAgentDescriptors([
    { id: "claude", display_name: "Claude Code", supports_chat_ui: true } as ProviderDescriptor,
  ]);
  const peers: MeshPeersResponse = {
    local_instance_name: "Local",
    local_machine_identity: { kind: "desktop" },
    machine_identities: {},
    installations: [],
    paired: [{ instance_id: "eight", instance_name: "eight", online: true }] as MeshPeersResponse["paired"],
    local_dev: false,
    local_last_active_ms: null,
  };
  rememberMachineIdentities(peers);
  updateRemoteChatPeers(peers);

  const styleEl = document.createElement("style");
  styleEl.textContent = `${PRIMITIVES_CSS}\n${TAB_TOP_BAR_CSS}`;
  document.head.append(styleEl);

  const host = document.createElement("div");
  host.style.width = "360px";
  document.body.append(host);
  const root = createRoot(host);
  const store = createMuxStateStore(() => makeState(tab));
  const actions = { upsertToast: vi.fn() } as unknown as MultiplexerActionsContextType;

  try {
    await act(async () => {
      root.render(
        <MultiplexerActionsContext.Provider value={actions}>
          <MuxStateStoreContext.Provider value={store}>
            <TabTopBar tabId={tab.id} shellTab={tab} cwd={tab.cwd} showSwitch isActive />
          </MuxStateStoreContext.Provider>
        </MultiplexerActionsContext.Provider>,
      );
    });

    const bar = host.querySelector<HTMLElement>('[data-testid="tab-top-bar"]');
    const identity = bar?.querySelector<HTMLElement>(".ct-tab-top-bar__identity");
    const switcher = identity?.querySelector<HTMLElement>('[data-testid="tab-mode-switch"]');
    expect(switcher).not.toBeNull();
    expect(bar?.querySelector('[data-testid="remote-viewer-pill"]')).toBeNull();
    if (tab.remote_peer_id) {
      expect(identity?.querySelector(".ct-tab-header-title__subtitle")?.textContent).toContain("·");
    }

    const switcherStyle = getComputedStyle(switcher!);
    const switcherWidth = resolvePxLength(switcherStyle.width, switcher!);
    const switcherHeight = resolvePxLength(switcherStyle.height, switcher!);
    expect(switcherWidth).toBe(48);
    expect(switcherHeight).toBe(26);
    expect(switcherStyle.flexShrink).toBe("0");

    const chatBtn = switcher!.querySelector<HTMLButtonElement>('[role="tab"][data-value="chat"]');
    const termBtn = switcher!.querySelector<HTMLButtonElement>('[role="tab"][data-value="terminal"]');
    expect(chatBtn).not.toBeNull();
    expect(termBtn).not.toBeNull();

    for (const button of [chatBtn!, termBtn!]) {
      const btnStyle = getComputedStyle(button);
      const cellWidth = resolvePxLength(btnStyle.width, button);
      const cellMinWidth = resolvePxLength(btnStyle.minWidth, button);
      expect(cellWidth).toBeGreaterThan(0);
      expect(cellWidth).toBe(22);
      expect(cellMinWidth).toBe(22);
      expect(btnStyle.flexShrink).toBe("0");
    }

    await act(async () => {
      termBtn!.click();
    });
    const thumb = switcher!.querySelector<HTMLElement>(".ct-liquid-segmented__thumb");
    expect(thumb?.classList.contains("ct-liquid-segmented__thumb--travel")).toBe(true);
    expect(thumb?.style.getPropertyValue("--sx-from")).toBe("0%");
    expect(thumb?.style.getPropertyValue("--sx-to")).toBe("100%");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    styleEl.remove();
    resetRemoteChatStatus();
  }
});

it("leaves standard label Segmented controls auto-sized", async () => {
  const styleEl = document.createElement("style");
  styleEl.textContent = PRIMITIVES_CSS;
  document.head.append(styleEl);

  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);

  try {
    await act(async () => {
      root.render(
        <Segmented
          value="diff"
          height={26}
          options={[
            { value: "diff", label: "Diff" },
            { value: "file", label: "File" },
          ]}
          onChange={() => {}}
          data-testid="label-segmented"
        />,
      );
    });

    const control = host.querySelector<HTMLElement>('[data-testid="label-segmented"]');
    expect(control).not.toBeNull();
    expect(control!.classList.contains("ct-liquid-segmented--icon-only")).toBe(false);
    expect(resolvePxLength(getComputedStyle(control!).width, control!)).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    styleEl.remove();
  }
});
