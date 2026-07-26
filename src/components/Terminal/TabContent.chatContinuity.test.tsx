// Run: npx tsx src/components/Terminal/TabContent.chatContinuity.test.tsx
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type {
  Pane,
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

function setupDom(): void {
  const hookWindow = new Window({ url: "http://localhost/" });
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

function boundPane(): Pane {
  return {
    id: "pane-live",
    pty_id: "pty-live",
    backing: { kind: "pty" },
    pane_type: "Terminal",
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

function stateFor(pane: Pane, generation: number): ExtendedState {
  return {
    workspaces: [{
      id: "workspace",
      name: "Project",
      cwd: "D:\\work\\project",
      tabs: [{ id: "tab", pane_id: pane.id }],
      active_tab_id: "tab",
    }],
    panes: { [pane.id]: pane },
    teams: [],
    active_workspace_id: "workspace",
    focused_pane_id: pane.id,
    notifications: [],
    paneStatuses: {},
    toasts: [],
    activeTabIds: { workspace: "tab" },
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
      if (property === "selectTabByPane") return noopAsync;
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
      if (command === "list_agent_providers") return [descriptor];
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

async function main(): Promise<void> {
  setupDom();
  const bridgeModule = await import("../../lib/bridge");
  const { setAgentDescriptors } = await import("../../lib/agentDescriptorCache");
  const { __resetPluginSnapshotStoreForTest } = await import("../../lib/pluginSnapshotStore");
  const { createRequire } = await import("node:module");
  const require = createRequire(__filename);
  require.extensions[".css"] = () => {};
  const { TabContent } = await import("./TabContent");
  setAgentDescriptors([descriptor]);
  __resetPluginSnapshotStoreForTest();

  const server = new FakeBridge();
  server.install(bridgeModule as never);
  let state = stateFor(boundPane(), 1);
  const store = createMuxStateStore(() => state);
  const host = document.createElement("main");
  document.body.appendChild(host);
  const root = createRoot(host);

  const render = async () => {
    await act(async () => {
      root.render(
        <MuxStateStoreContext.Provider value={store}>
          <MultiplexerActionsContext.Provider value={muxActions()}>
            <TabContent
              pane={state.panes["pane-live"]}
              isFocused
              onFocusPane={() => {}}
              onPtySpawned={() => {}}
              theme={theme}
              isVisible
              chatFirst={false}
              chatDefaultPrompted
            />
          </MultiplexerActionsContext.Provider>
        </MuxStateStoreContext.Provider>,
      );
    });
    await flush();
  };

  await render();
  const switchBeforeRefresh = host.querySelector('[data-testid="pane-mode-switch"]');
  assert(switchBeforeRefresh, "bound provider did not expose the Terminal/Chat switch");
  assert(
    switchBeforeRefresh.querySelector('[data-value="terminal"]')?.getAttribute("aria-selected") === "true",
    "continuity harness did not start in Terminal",
  );
  assert(server.starts.length === 0, "Terminal requested a Chat snapshot");

  state = stateFor({ ...boundPane() }, 2);
  await act(async () => {
    store.notify();
  });
  await render();
  assert(
    host.querySelector('[data-testid="pane-mode-switch"]'),
    "snapshot refresh lost the provider-owned Chat capability",
  );
  assert(server.starts.length === 0, "same-authority refresh attached hidden Chat");

  const chatTab = host.querySelector(
    '[data-testid="pane-mode-switch"] [data-value="chat"]',
  ) as HTMLButtonElement | null;
  assert(chatTab, "Chat switch action disappeared after refresh");
  await act(async () => {
    chatTab.click();
  });
  await flush();

  assert(Number(server.starts.length) === 1, `Terminal -> Chat started ${server.starts.length} snapshots`);
  assert(
    server.starts[0]?.agentSessionId === "sid-live",
    "Terminal -> Chat lost the pane-owned session id",
  );
  assert(Number(server.subscribers.size) === 1, "Terminal -> Chat did not own one subscription");
  const rows = host.querySelectorAll('[data-testid="chat-message"]');
  assert(rows.length === 1, `bounded transcript rendered ${rows.length} rows`);
  assert(rows[0]?.textContent?.includes("pre-existing transcript"), "bounded transcript row is missing");
  assert(
    !host.querySelector('[data-testid="chat-setup-state"]'),
    "hydrated transcript remained covered by stale ShellReady",
  );
  const agentState = host.querySelector('[data-testid="agent-state"]');
  assert(agentState?.textContent?.includes("Bound Provider"), "provider identity was not preserved");
  assert(agentState?.textContent?.includes("model-live"), "model identity was not preserved");
  assert(
    host.querySelector('[data-testid="pane-mode-switch"] [data-value="chat"]')
      ?.getAttribute("aria-selected") === "true",
    "Terminal -> Chat switch did not retain Chat mode",
  );

  await act(async () => {
    root.unmount();
  });
  await flush();
  assert(Number(server.subscribers.size) === 0, "unmount retained the Chat subscription");
  assert(server.stops.length === 1, "unmount did not stop exactly one subscription");
  console.log(
    "PASS TabContent continuity: provider/model/switch + one transcript/subscription survived refresh and Terminal -> Chat",
  );
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
