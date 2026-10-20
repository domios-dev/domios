import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatSetupState } from "./ChatSetupState";
import { ChatComposerSurface } from "./ChatComposerSurface";
import { chatSetupView } from "./chatSetupModel";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import type { Tab, Workspace } from "../../lib/types";

const win = new Window({ url: "http://localhost" });
Object.defineProperty(globalThis, "window", { value: win, configurable: true });
Object.defineProperty(globalThis, "document", { value: win.document, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: win.HTMLElement, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function main() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const tab = { id: "failed", cwd: "/missing/folder", boot_stage: "spawn_failed", boot_failure_reason: JSON.stringify({ code: 'workspace_cwd_missing', raw: 'opaque-folder-error' }) } as Tab;
  const workspace = { id: "workspace", name: "Fixture", cwd: tab.cwd } as Workspace;
  let closed = 0;
  const actions = { setWorkspaceCwd: async () => {}, closeTab: () => { closed++; } } as unknown as MultiplexerActionsContextType;
  const render = (current: Tab) => {
    const view = chatSetupView(current);
    return <MultiplexerActionsContext.Provider value={actions}>
      <ChatSetupState tab={current} workspace={workspace} view={view} providerName="Fixture" />
      <ChatComposerSurface view={view}><textarea aria-label="Message" /></ChatComposerSurface>
    </MultiplexerActionsContext.Provider>;
  };
  try {
    await act(async () => { root.render(render(tab)); });
    const choose = [...host.querySelectorAll("button")].find(button => button.textContent === "Choose folder");
    assert(choose, "primary recovery action has its accessible name");
    assert.equal(document.activeElement, choose, "primary recovery action receives initial focus");
    assert.equal(host.querySelector('textarea, [role="textbox"]'), null);
    assert.equal(host.querySelector("details")?.open, false);
    const close = [...host.querySelectorAll("button")].find(button => button.textContent === "Close tab");
    assert(close);
    await act(async () => { close.click(); });
    assert.equal(closed, 1);
    await act(async () => { root.render(render({ ...tab, boot_stage: "agent_ready" })); });
    assert(host.querySelector('textarea[aria-label="Message"]'));
  } finally {
    await act(async () => { root.unmount(); });
    host.remove();
  }
  console.log("✓ launch failure wiring: recovery focus, close action, disclosure, composer hidden and restored");
}
void main();
