import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Tab, Workspace } from "../../lib/types";

const invokeMock = vi.hoisted(() => vi.fn(async (..._args: unknown[]) => ({})));
vi.mock("../../lib/bridge", () => ({ invoke: invokeMock }));

import { EssenceMapCanvasInner } from "./EssenceMapCanvas";
import { askOrchestratorToBuildMap, findWorkspaceOrchestrator, type MapOrchestrator } from "./essenceMapOrchestrator";
import type { EssenceMapFile } from "./essenceMapModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const workspace: Workspace = { id: "ws-map", name: "Workspace", tabs: ["tab-orch"], active_tab_id: null, cwd: "/repo/a" };
const missingMap: EssenceMapFile = { project_path: "/repo/a", map_path: "/repo/a/essences/map.jsonl", status: "missing" };
const orchestrator: MapOrchestrator = { tabId: "tab-orch", title: "Fermi" };

const mounted: Array<{ host: HTMLDivElement; root: Root }> = [];

beforeEach(() => {
  document.body.replaceChildren();
  invokeMock.mockClear();
});

afterEach(async () => {
  await act(async () => {
    for (const { root } of mounted) root.unmount();
  });
  mounted.length = 0;
});

async function mount(element: ReactNode): Promise<HTMLDivElement> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ host, root });
  await act(async () => root.render(element));
  return host;
}

function emptyState(host: HTMLElement): HTMLElement {
  const section = host.querySelector<HTMLElement>('[data-testid="essence-map-no-nodes"]');
  if (!section) throw new Error("empty state not rendered");
  return section;
}

function askButton(host: HTMLElement): HTMLButtonElement | null {
  return host.querySelector<HTMLButtonElement>('[data-testid="essence-map-ask-orchestrator"]');
}

function tab(id: string, overrides: Partial<Tab>): Tab {
  return { id, tab_type: "Terminal", title: id, cwd: null, agent_type: "claude", ...overrides } as Tab;
}

describe("Map empty state", () => {
  it("replaces the panned world with a labelled, described section when no map has nodes", async () => {
    const host = await mount(<EssenceMapCanvasInner workspace={workspace} mapFiles={[missingMap]} orchestrator={orchestrator} onAskOrchestrator={vi.fn(async () => {})} />);
    const section = emptyState(host);
    expect(section.tagName).toBe("SECTION");
    const title = document.getElementById(section.getAttribute("aria-labelledby") ?? "");
    const description = document.getElementById(section.getAttribute("aria-describedby") ?? "");
    expect(title?.textContent?.trim()).toBeTruthy();
    expect(description?.textContent).toContain(orchestrator.title);
    expect(host.querySelector('[data-testid="essence-map-project-missing"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="essence-map-transform-layer"]')).toBeNull();
  });

  it("puts the Orchestrator action first in focus order and fires it once per click burst", async () => {
    let release: () => void = () => {};
    const onAsk = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
    const onRetry = vi.fn();
    const host = await mount(<EssenceMapCanvasInner workspace={workspace} mapFiles={[missingMap]} orchestrator={orchestrator} onAskOrchestrator={onAsk} onRetry={onRetry} />);
    const buttons = Array.from(emptyState(host).querySelectorAll("button"));
    const button = askButton(host);
    expect(button).not.toBeNull();
    expect(buttons[0]).toBe(button);
    expect(button?.type).toBe("button");
    expect(button?.textContent?.toLowerCase()).toContain("orchestrator");
    await act(async () => {
      button?.click();
      button?.click();
    });
    expect(onAsk).toHaveBeenCalledTimes(1);
    expect(onAsk).toHaveBeenCalledWith(orchestrator);
    expect(emptyState(host).getAttribute("aria-busy")).toBe("true");
    await act(async () => release());
    expect(emptyState(host).getAttribute("aria-busy")).toBe("false");
    expect(onRetry).not.toHaveBeenCalled();
  });

  it("announces a failed request as an alert and keeps the action available", async () => {
    const onAsk = vi.fn(async () => { throw new Error("tab gone"); });
    const host = await mount(<EssenceMapCanvasInner workspace={workspace} mapFiles={[missingMap]} orchestrator={orchestrator} onAskOrchestrator={onAsk} />);
    await act(async () => askButton(host)?.click());
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("tab gone");
    expect(askButton(host)?.disabled).toBe(false);
  });

  it("shows guidance without an action when the workspace has no Orchestrator", async () => {
    const host = await mount(<EssenceMapCanvasInner workspace={workspace} mapFiles={[missingMap]} orchestrator={null} onAskOrchestrator={vi.fn(async () => {})} />);
    const section = emptyState(host);
    expect(askButton(host)).toBeNull();
    expect(document.getElementById(section.getAttribute("aria-describedby") ?? "")?.textContent?.trim()).toBeTruthy();
  });
});

describe("findWorkspaceOrchestrator", () => {
  it("picks the first live Orchestrator in workspace order", () => {
    const tabs: Record<string, Tab> = {
      planner: tab("planner", { role: "Manager", role_profile: "planner" }),
      dead: tab("dead", { role: "Manager", role_profile: "orchestrator", agent_type: null }),
      general: tab("general", { role: "Manager", role_profile: "general_agent" }),
      live: tab("live", { role: "Manager", role_profile: "orchestrator", title: "Fermi" }),
      later: tab("later", { role: "Manager", role_profile: "orchestrator" }),
    };
    expect(findWorkspaceOrchestrator({ tabs: ["planner", "dead", "general", "live", "later"] }, tabs)).toEqual({ tabId: "live", title: "Fermi" });
    expect(findWorkspaceOrchestrator({ tabs: ["planner", "dead", "general"] }, tabs)).toBeNull();
  });
});

describe("askOrchestratorToBuildMap", () => {
  it("sends one request naming the projects through chat send, then focuses the Orchestrator tab", async () => {
    const focused = vi.fn();
    const onFocus = (event: Event) => focused((event as CustomEvent).detail);
    window.addEventListener("codeterm:focus-tab", onFocus);
    try {
      await askOrchestratorToBuildMap("ws-map", orchestrator, ["/repo/a", "/repo/b"]);
    } finally {
      window.removeEventListener("codeterm:focus-tab", onFocus);
    }
    expect(invokeMock).toHaveBeenCalledTimes(1);
    const [command, args] = invokeMock.mock.calls[0] as [string, { tabId: string; text: string }];
    expect(command).toBe("chat_send_text");
    expect(args.tabId).toBe("tab-orch");
    expect(args.text).toContain("/repo/a");
    expect(args.text).toContain("/repo/b");
    expect(focused).toHaveBeenCalledTimes(1);
    expect(focused).toHaveBeenCalledWith({ tabId: "tab-orch", workspaceId: "ws-map" });
  });

  it("does not focus the tab when the send is refused", async () => {
    invokeMock.mockRejectedValueOnce(new Error("refused"));
    const focused = vi.fn();
    window.addEventListener("codeterm:focus-tab", focused);
    try {
      await expect(askOrchestratorToBuildMap("ws-map", orchestrator, ["/repo/a"])).rejects.toThrow("refused");
    } finally {
      window.removeEventListener("codeterm:focus-tab", focused);
    }
    expect(focused).not.toHaveBeenCalled();
  });
});
