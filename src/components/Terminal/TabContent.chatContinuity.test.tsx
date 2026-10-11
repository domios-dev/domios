// Run: npx tsx src/components/Terminal/TabContent.chatContinuity.test.tsx
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type {
  Tab,
  ProviderDescriptor,
  ThemeConfig,
} from "../../lib/types";
import {
  MultiplexerActionsContext,
  MuxStateStoreContext,
  type MultiplexerActionsContextType,
} from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

function setupDom(): Window {
  const hookWindow = new Window({ url: "http://localhost/" });
  Object.assign(hookWindow, { __CODETERM_DAEMON__: { origin: hookWindow.location.origin, token: "fixture-token" } });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", {
    configurable: true,
    value: true,
  });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLTextAreaElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
    "CustomEvent",
    "localStorage",
    "sessionStorage",
  ] as const) {
    Object.defineProperty(globalThis, key, {
      configurable: true,
      value: (hookWindow as never)[key],
    });
  }
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    value: () => 1,
  });
  Object.defineProperty(globalThis, "cancelAnimationFrame", {
    configurable: true,
    value: () => {},
  });
  class StubResizeObserver {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    value: StubResizeObserver,
  });
  return hookWindow;
}

const descriptor: ProviderDescriptor = {
  schema_version: 1,
  id: "bound-provider",
  display_name: "Bound Provider",
  transport: "pty",
  system_prompt_delivery: "external",
  icon_hint: "BP",
  icon_color: "#4f8cff",
  available_models: [],
  settings_schema: [],
  notifications_schema: [],
  supports_lazy_skills: false,
  has_hooks: false,
  supports_chat_ui: true,
};

function boundTab(): Tab {
  return {
    id: "tab-live",
    pty_id: "pty-live",
    backing: { kind: "pty" },
    tab_type: "Terminal",
    title: "Planner",
    cwd: "D:\\work\\project",
    agent_type: descriptor.id,
    agent_session_id: "sid-live",
    session_binding_nonce: "nonce-live",
    agent_session_model: "model-live",
    agent_state: {
      state: "working",
      status_line: "Planning",
      phase: "thinking",
    },
    boot_stage: "shell_ready",
    protected: true,
    chat_view: false,
  };
}

function stateFor(tab: Tab, generation: number): ExtendedState {
  return {
    workspaces: [{
      id: "workspace",
      name: "Project",
      cwd: "D:\\work\\project",
      tabs: [tab.id],
      active_tab_id: tab.id,
    }],
    tabs: { [tab.id]: tab },
    teams: [],
    active_workspace_id: "workspace",
    focused_tab_id: tab.id,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: { workspace: tab.id },
    generation,
    pendingCloses: [],
  } as ExtendedState;
}

function muxActions(): MultiplexerActionsContextType {
  const noop = () => {};
  const noopAsync = async () => undefined;
  return new Proxy({}, {
    get: (_target, property) => {
      if (property === "spawnAgentTab") return async () => null;
      if (property === "createOrchestrationPair") {
        return async () => {
          throw new Error("not implemented");
        };
      }
      if (property === "createWorkspace" || property === "createTab") return noopAsync;
      if (property === "selectTabByTab") return noopAsync;
      return noop;
    },
  }) as MultiplexerActionsContextType;
}

type BridgeHandler = (event: { payload: unknown }) => void;

class FakeBridge {
  readonly starts: Array<Record<string, unknown>> = [];
  readonly stops: string[] = [];
  readonly subscribers = new Set<string>();
  readonly handlers = new Map<string, Set<BridgeHandler>>();

  install(bridge: {
    __setInvokeForTest: (fn: unknown) => void;
    __setListenForTest: (fn: unknown) => void;
  }): void {
    bridge.__setListenForTest(async (event: string, handler: BridgeHandler) => {
      const handlers = this.handlers.get(event) ?? new Set<BridgeHandler>();
      handlers.add(handler);
      this.handlers.set(event, handlers);
      return () => {
        handlers.delete(handler);
      };
    });
    bridge.__setInvokeForTest(async (command: string, args: Record<string, unknown> = {}) => {
      if (command === "list_agent_providers") {
        const { webJson } = await import("../../lib/webApi");
        return webJson("GET", "/api/agents/providers");
      }
      if (command === "plugin_manager_snapshot") {
        return { plugins: [], channels: [], diff: [], marketplace: [] };
      }
      if (command === "delayed_send_list") return [];
      if (command === "chat_start") {
        this.starts.push(args);
        const subscriptionId = String(args.subscriptionId);
        this.subscribers.add(subscriptionId);
        return {
          text: "",
          start_offset: 0,
          offset: 1,
          pending: false,
          messages: [{
            id: "history",
            type: "assistant",
            content: "pre-existing transcript",
            timestamp: "1",
          }],
          source_session_id: "sid-live",
          session_binding_nonce: "nonce-live",
          subscription_id: subscriptionId,
        };
      }
      if (command === "chat_stop") {
        const subscriptionId = String(args.subscriptionId);
        this.stops.push(subscriptionId);
        this.subscribers.delete(subscriptionId);
      }
      return {};
    });
  }
}

const theme = {
  name: "test",
  background: "#000",
  foreground: "#fff",
} as ThemeConfig;

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function settleUntil(done: () => boolean, label: string): Promise<void> {
  for (let turn = 0; turn < 50 && !done(); turn++) {
    await act(async () => {
      await new Promise<void>((resolve) => setImmediate(resolve));
    });
  }
  assert(done(), label);
}

async function main(): Promise<void> {
  const hookWindow = setupDom();
  const bridgeModule = await import("../../lib/bridge");
  const { setAgentDescriptors } = await import("../../lib/agentDescriptorCache");
  const { __resetPluginSnapshotStoreForTest } = await import("../../lib/pluginSnapshotStore");
  const { createRequire } = await import("node:module");
  const require = createRequire(__filename);
  require.extensions[".css"] = () => {};
  const { TabContent } = await import("./TabContent");
  setAgentDescriptors([]);
  const { useAgentProviderCachePublisher } = await import("../../hooks/useAgentProviders");
  const { setRelayTarget, invalidateWebRequestContext } = await import("../../lib/webApi");
  const catalogRequests: string[] = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init) => {
    catalogRequests.push(String(input));
    if (String(input).endsWith("/api/mesh/relay-to")) {
      const call = JSON.parse(String(init?.body));
      assert(call.target_peer_id === "owning-peer", "catalog requested from wrong peer");
      assert(call.path === "/api/agents/providers", "catalog used wrong relay route");
      return Response.json({ ok: true, status: 200, data: [descriptor], error: null });
    }
    return Response.json([]);
  };
  function CatalogPublisher() {
    useAgentProviderCachePublisher();
    return null;
  }
  __resetPluginSnapshotStoreForTest();

  const server = new FakeBridge();
  server.install(bridgeModule as never);
  let state = stateFor(boundTab(), 1);
  const store = createMuxStateStore(() => state);
  const host = document.createElement("main");
  document.body.appendChild(host);
  const root = createRoot(host);

  const render = async () => {
    await act(async () => {
      root.render(
        <MuxStateStoreContext.Provider value={store}>
          <MultiplexerActionsContext.Provider value={muxActions()}>
            <CatalogPublisher />
            <TabContent
              tab={state.tabs["tab-live"]}
              isFocused
              onFocusTab={() => {}}
              onPtySpawned={() => {}}
              theme={theme}
              isVisible
            />
          </MultiplexerActionsContext.Provider>
        </MuxStateStoreContext.Provider>,
      );
    });
    await flush();
  };

  await render();
  assert(!host.querySelector('[data-testid="tab-mode-switch"]'), "gateway catalog invented a peer capability");
  assert(host.querySelector('[data-testid="provider-capability-state"]'), "missing descriptor is invisible");
  await act(async () => {
    invalidateWebRequestContext();
    window.dispatchEvent(new CustomEvent("codeterm-peer-switch-reset"));
  });
  setRelayTarget("owning-peer", "Owning peer");
  invalidateWebRequestContext();
  await act(async () => {
    window.dispatchEvent(new CustomEvent("codeterm-peer-switch-rebind"));
  });
  await settleUntil(() => catalogRequests.some((url) => url.endsWith("/api/mesh/relay-to")), "peer switch did not refresh catalog");
  await settleUntil(() => !host.querySelector('[data-testid="provider-capability-state"]'), "resolved descriptor retained missing state");
  const switchBeforeRefresh = host.querySelector('[data-testid="tab-mode-switch"]');
  assert(switchBeforeRefresh, "bound provider did not expose the Terminal/Chat switch");
  assert(
    switchBeforeRefresh.querySelector('[data-value="chat"]')?.getAttribute("aria-selected") === "true",
    "chat-capable Tab did not start in Chat",
  );
  assert(server.starts.length === 1, `default Chat started ${server.starts.length} snapshots`);
  assert(server.starts[0]?.agentSessionId === "sid-live", "default Chat lost the tab-owned session id");
  assert(Number(server.subscribers.size) === 1, "default Chat did not own one subscription");

  state = stateFor({ ...boundTab() }, 2);
  await act(async () => {
    store.notify();
  });
  await render();
  assert(
    host.querySelector('[data-testid="tab-mode-switch"]'),
    "snapshot refresh lost the provider-owned Chat capability",
  );
  assert(Number(server.starts.length) === 1, "same-authority refresh restarted Chat");
  assert(
    server.starts[0]?.agentSessionId === "sid-live",
    "same-authority refresh lost the tab-owned session id",
  );
  assert(Number(server.subscribers.size) === 1, "same-authority refresh duplicated the subscription");
  const rows = host.querySelectorAll('[data-testid="chat-message"]');
  assert(rows.length === 1, `bounded transcript rendered ${rows.length} rows`);
  assert(rows[0]?.textContent?.includes("pre-existing transcript"), "bounded transcript row is missing");
  assert(
    !host.querySelector('[data-testid="chat-setup-state"]'),
    "hydrated transcript remained covered by stale ShellReady",
  );
  const agentState = host.querySelector('[data-testid="agent-state"]');
  assert(agentState?.textContent?.includes("model-live"), "model identity was not preserved");
  assert(
    host.querySelector('[data-testid="tab-mode-switch"] [data-value="chat"]')
      ?.getAttribute("aria-selected") === "true",
    "same-authority refresh did not retain Chat mode",
  );

  const terminalControl = host.querySelector<HTMLElement>('[data-testid="tab-mode-switch"] [data-value="terminal"]');
  assert(terminalControl, "remote descriptor did not expose Terminal selection");
  await act(async () => terminalControl.click());
  await flush();
  assert(terminalControl.getAttribute("aria-selected") === "true", "Terminal selection was not retained");
  const frame = host.querySelector<HTMLElement>('[data-testid="terminal-frame"]');
  assert(frame?.getAttribute("data-terminal-visible") === "true", "remote terminal frame stayed inactive");
  assert(frame.style.visibility === "visible", "remote terminal frame stayed hidden");

  await act(async () => {
    root.unmount();
  });
  await flush();
  bridgeModule.__setInvokeForTest(null);
  bridgeModule.__setListenForTest(null);
  globalThis.fetch = originalFetch;
  await hookWindow.happyDOM.abort();
  hookWindow.close();
  assert(Number(server.subscribers.size) === 0, "unmount retained the Chat subscription");
  assert(server.stops.length === 1, "unmount did not stop exactly one subscription");
  console.log(
    "PASS TabContent continuity: default Chat provider/model + one transcript/subscription survived refresh",
  );
  process.exit(0);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
