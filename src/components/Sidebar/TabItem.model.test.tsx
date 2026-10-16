// Run: npx tsx src/components/Sidebar/TabItem.model.test.tsx

import { Window } from "happy-dom";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
Object.defineProperty(globalThis, "HTMLInputElement", { value: window.HTMLInputElement, configurable: true });
Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
// happy-dom's timers are the Node ones (BrowserWindow.d.ts:1297,1303), so the
// handle is a Timeout, never a number.
type TimerHandle = ReturnType<typeof window.setTimeout>;
Object.defineProperty(globalThis, "requestAnimationFrame", {
  value: (cb: (t: number) => void): TimerHandle => window.setTimeout(() => cb(0), 0),
  configurable: true,
});
Object.defineProperty(globalThis, "cancelAnimationFrame", {
  value: (id: TimerHandle) => window.clearTimeout(id),
  configurable: true,
});
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type ProviderDescriptor = import("../../lib/types").ProviderDescriptor;
type Tab = import("../../lib/types").Tab;

async function main() {
  const [{ act }, { createRoot }, { TabItem }, { setAgentDescriptors }] = await Promise.all([
    import("react"), import("react-dom/client"), import("./TabItem"), import("../../lib/agentDescriptorCache"),
  ]);
  setAgentDescriptors([{
    id: "fixture-provider", display_name: "Fixture", transport: "pty",
    available_models: [], agent_runtime: { fastMode: true },
  } as unknown as ProviderDescriptor]);
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host as unknown as HTMLElement);
  const tab: Tab = {
    id: "tab-agent", pty_id: "pty-agent", tab_type: "Terminal", title: "Agent", cwd: null,
    agent_type: "fixture-provider", agent_session_model: "fixture-model", boot_stage: "agent_ready",
    agent_session_id: "own-thread", session_binding_nonce: "own-nonce",
  };
  const render = async (fast?: boolean) => {
    await act(async () => root.render(<TabItem tab={{ ...tab, agent_runtime_config: {
      fastMode: true,
      ...(fast === undefined ? {} : { confirmedFastMode: { enabled: fast, sessionId: "own-thread", bindingNonce: "own-nonce" } }),
    } }} index={0} isActive={false} onClick={() => undefined} onClose={() => undefined}
      canClose={false} workspaceId="workspace" hasAgent status="Idle" />));
    return host.querySelector('[data-testid="tab-subline"]')?.textContent ?? "";
  };
  try {
    for (const fast of [true, false, undefined]) {
      const label = await render(fast);
      if (!label.includes("fixture-model") || label.includes(" · fast") !== (fast === true)) {
        throw new Error(`Row did not reflect confirmed runtime tier ${fast}: ${label}`);
      }
    }
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
  console.log("TabItem model and confirmed fast tier wiring: 3 states passed");
}

void main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
