// Run: npx tsx src/components/Terminal/TabModeSwitch.test.tsx

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import { publishCurrentTabUiMode } from "../../lib/clientTabUiMode";
import { isTabUiModeEvent, type TabUiMode } from "../../lib/chatUiMode";
import { TabModeSwitch } from "./TabModeSwitch";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

const win = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
for (const key of [
  "window",
  "document",
  "navigator",
  "HTMLElement",
  "SVGElement",
  "Element",
  "Node",
  "Event",
  "CustomEvent",
  "localStorage",
  "sessionStorage",
] as const) {
  Object.defineProperty(globalThis, key, { configurable: true, value: (win as never)[key] });
}
Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: () => 1 });

function makeState(tab: Tab): ExtendedState {
  return {
    workspaces: [{ id: "ws-1", name: "WS", root_path: "/tmp", tab_ids: [tab.id], split_layouts: {}, active_tab_id: tab.id }],
    tabs: { [tab.id]: tab },
    active_workspace_id: "ws-1",
    active_tab_id: tab.id,
    seq: 1,
    loading: false,
    connected: true,
    error: null,
  } as unknown as ExtendedState;
}

function makeTab(id: string, overrides: Partial<Tab> = {}): Tab {
  return {
    id,
    title: "Agent",
    cwd: "/tmp",
    agent_type: "chat-agent",
    ...overrides,
  } as Tab;
}

async function main(): Promise<void> {
  setAgentDescriptors([
    { id: "chat-agent", display_name: "Chat Agent", supports_chat_ui: true } as ProviderDescriptor,
    { id: "pty-only", display_name: "PTY Only", supports_chat_ui: false } as ProviderDescriptor,
  ]);

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);

  const emitted: Array<{ tabId: string; mode: TabUiMode }> = [];
  const onModeEvent = (event: Event) => {
    if (isTabUiModeEvent(event)) emitted.push(event.detail);
  };
  window.addEventListener("codeterm-tab-ui-mode", onModeEvent);

  const renderSwitch = async (tab: Tab, chatPty = true) => {
    let state = makeState(tab);
    const store = createMuxStateStore(() => state);
    await act(async () => {
      root.render(
        <MuxStateStoreContext.Provider value={store}>
          <TabModeSwitch key={`${tab.id}-${String(chatPty)}`} tabId={tab.id} chatPty={chatPty} />
        </MuxStateStoreContext.Provider>,
      );
    });
  };

  await renderSwitch(makeTab("tab-hidden"), false);
  assert(host.querySelector('[data-testid="tab-mode-switch"]') === null, "chatPty=false hides switch");

  await renderSwitch(makeTab("tab-shell", { agent_type: null }));
  assert(
    host.querySelector('[data-testid="tab-mode-switch"] [data-value="terminal"]')?.getAttribute("aria-selected") === "true",
    "plain shell tab defaults to terminal",
  );

  await renderSwitch(makeTab("tab-pty-only", { agent_type: "pty-only" }));
  assert(
    host.querySelector('[data-testid="tab-mode-switch"] [data-value="terminal"]')?.getAttribute("aria-selected") === "true",
    "non-chat provider defaults to terminal",
  );

  await renderSwitch(makeTab("tab-backend", { agent_type: null, backing: { kind: "chat_backend", plugin_id: "demo" } as Tab["backing"] }));
  assert(
    host.querySelector('[data-testid="tab-mode-switch"] [data-value="chat"]')?.getAttribute("aria-selected") === "true",
    "chat_backend defaults to chat",
  );

  await renderSwitch(makeTab("tab-chat"));
  const switcher = host.querySelector<HTMLElement>('[data-testid="tab-mode-switch"]');
  assert(switcher, "renders tab-mode-switch");
  assert(switcher.getAttribute("role") === "tablist", "uses shared Segmented tablist role");
  assert(switcher.getAttribute("aria-label") === "View", "preserves View accessible label");
  assert(switcher.getAttribute("data-tab-id") === "tab-chat", "preserves data-tab-id");
  assert(switcher.classList.contains("ct-liquid-segmented"), "uses shared liquid segmented class");
  assert(switcher.classList.contains("ct-liquid-segmented--icon-only"), "uses icon-only mode");
  assert(switcher.classList.contains("ct-tab-view-switch"), "preserves ct-tab-view-switch focus guard class");

  const chatBtn = switcher.querySelector<HTMLButtonElement>('[role="tab"][data-value="chat"]');
  const termBtn = switcher.querySelector<HTMLButtonElement>('[role="tab"][data-value="terminal"]');
  assert(chatBtn && termBtn, "renders Chat and Terminal tab buttons");
  assert(chatBtn.getAttribute("aria-label") === "Chat" && termBtn.getAttribute("aria-label") === "Terminal", "labels are accessible names");
  assert(chatBtn.title === "Chat" && termBtn.title === "Terminal", "tooltips are preserved");
  assert(chatBtn.textContent === "" && termBtn.textContent === "", "icon-only switch renders no visible text labels");
  assert(chatBtn.getAttribute("aria-selected") === "true" && chatBtn.tabIndex === 0 && termBtn.tabIndex === -1, "starts selected on Chat");

  await act(async () => {
    termBtn.click();
  });
  assert(termBtn.getAttribute("aria-selected") === "true", "click selects Terminal");
  assert(Number(emitted.length) === 1 && emitted[0].tabId === "tab-chat" && emitted[0].mode === "terminal", "click emits codeterm-tab-ui-mode");
  const thumbToTerm = switcher.querySelector<HTMLElement>(".ct-liquid-segmented__thumb");
  assert(thumbToTerm, "thumb renders after click");
  assert(thumbToTerm.classList.contains("ct-liquid-segmented__thumb--travel"), "click triggers liquid travel animation");
  assert(thumbToTerm.style.getPropertyValue("--sx-from") === "0%" && thumbToTerm.style.getPropertyValue("--sx-to") === "100%", "travel goes chat -> terminal");

  const key = (code: string) => new win.KeyboardEvent("keydown", { code, bubbles: true, cancelable: true });
  await act(async () => {
    termBtn.focus();
    termBtn.dispatchEvent(key("ArrowLeft") as unknown as Event);
  });
  assert(chatBtn.getAttribute("aria-selected") === "true" && document.activeElement === chatBtn, "ArrowLeft selects and focuses Chat");
  assert(Number(emitted.length) === 2 && emitted[1].mode === "chat", "ArrowLeft emits chat mode");
  const thumbToChat = switcher.querySelector<HTMLElement>(".ct-liquid-segmented__thumb");
  assert(thumbToChat, "thumb renders after arrow key");
  assert(thumbToChat.classList.contains("ct-liquid-segmented__thumb--travel"), "arrow key triggers liquid travel animation");
  assert(thumbToChat.style.getPropertyValue("--sx-from") === "100%" && thumbToChat.style.getPropertyValue("--sx-to") === "0%", "travel goes terminal -> chat");

  await act(async () => {
    publishCurrentTabUiMode("tab-chat", "terminal");
  });
  assert(termBtn.getAttribute("aria-selected") === "true", "listens to published current tab ui mode");

  window.removeEventListener("codeterm-tab-ui-mode", onModeEvent);
  await act(async () => root.unmount());
  await win.happyDOM.abort();
  win.close();
  console.log("ok - TabModeSwitch reuses liquid icon-only Segmented");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
