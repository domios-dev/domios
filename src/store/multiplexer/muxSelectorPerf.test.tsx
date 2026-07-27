// Run: npx tsx src/store/multiplexer/muxSelectorPerf.test.tsx

export {};

async function setupHappyDom() {
  const React = await import("react");
  const { act, useLayoutEffect } = React;
  const { createRoot } = await import("react-dom/client");
  const { Window } = await import("happy-dom");

  const window = new Window({ url: "http://localhost:7686/" });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLButtonElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
    "CustomEvent",
  ] as const) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  process.env.VITE_CODETERM_TEST = "1";
  if (typeof (globalThis as { requestAnimationFrame?: unknown }).requestAnimationFrame !== "function") {
    (globalThis as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame = (
      cb: FrameRequestCallback,
    ) => {
      cb(0);
      return 0;
    };
    (globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};
  }
  if (typeof (globalThis as { ResizeObserver?: unknown }).ResizeObserver !== "function") {
    class StubResizeObserver {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    (globalThis as { ResizeObserver: unknown }).ResizeObserver = StubResizeObserver;
  }
  return { act, useLayoutEffect, React, createRoot, window };
}

async function main() {
  const { act, useLayoutEffect, React, createRoot, window } = await setupHappyDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { MultiplexerProvider } = await import("./index");
  const { TabSessionChat } = await import("../../components/Terminal/TabSessionChat");

  __setListenForTest(async () => () => {});
  __setInvokeForTest((async (cmd: string) => {
    if (cmd === "chat_start") return { text: "", offset: 0, messages: [], source_session_id: null, session_binding_nonce: null };
    if (cmd === "delayed_send_list") return [];
    if (cmd === "list_notifications") return [];
    if (cmd === "mux_get_state") {
      return {
        workspaces: [{
          id: "ws",
          name: "ws",
          tabs: [tabA, tabB],
          active_tab_id: tabA,
          cwd: null,
        }],
        tabs: {
          [tabA]: {
            id: tabA,
            pty_id: null,
            tab_type: "Terminal",
            title: "A",
            cwd: null,
            agent_type: "claude",
            chat_view: true,
            backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "s1" },
          },
          [tabB]: {
            id: tabB,
            pty_id: null,
            tab_type: "Terminal",
            title: "B",
            cwd: null,
            agent_type: "claude",
            chat_view: true,
            backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "s2" },
          },
        },
        active_workspace_id: "ws",
        focused_tab_id: tabA,
        generation: 1,
      };
    }
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);

  const tabA = "tab-a-churn";
  const tabB = "tab-b-hidden";

  function Boot() {
    useLayoutEffect(() => {
      window.__renderCounts = {};
    }, []);
    return (
      <MultiplexerProvider>
        <TabSessionChat tabId={tabA} chatBackend isVisible={false} />
        <TabSessionChat tabId={tabB} chatBackend isVisible={false} />
      </MultiplexerProvider>
    );
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(React.createElement(Boot));
  });
  for (let i = 0; i < 5; i += 1) {
    await act(async () => {
      await Promise.resolve();
    });
  }

  const { isCodeTermFrontendTestMode } = await import("../../components/Terminal/useChatSession");
  if (!isCodeTermFrontendTestMode()) {
    throw new Error("VITE_CODETERM_TEST not visible to isCodeTermFrontendTestMode");
  }

  if (!window.__muxPerfChurnTab) {
    throw new Error("__muxPerfChurnTab hook missing — VITE_CODETERM_TEST path broken");
  }

  const beforeB = window.__renderCounts?.[tabB] ?? 0;
  const beforeA = window.__renderCounts?.[tabA] ?? 0;

  for (let i = 0; i < 60; i += 1) {
    await act(async () => {
      window.__muxPerfChurnTab?.(tabA);
    });
  }

  const afterB = window.__renderCounts?.[tabB] ?? 0;
  const afterA = window.__renderCounts?.[tabA] ?? 0;
  const deltaB = afterB - beforeB;
  const deltaA = afterA - beforeA;

  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);
  await window.happyDOM.abort();
  window.close();

  if (deltaB > 3) {
    throw new Error(`hidden Tab B render delta ${deltaB} exceeds budget (pre-fix ~60)`);
  }
  if (deltaA <= 0) {
    throw new Error(`churn owner Tab A did not re-render (delta=${deltaA})`);
  }
  console.log(`PASS mux selector render isolation: tabA delta=${deltaA}, tabB delta=${deltaB}`);
  process.exit(0);
}

void main().catch((e) => {
  console.error("FAIL", e);
  process.exit(1);
});
