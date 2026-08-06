import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest } from "../../lib/bridge";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

function setupDom(): Window {
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  Object.defineProperty(globalThis, "window", { configurable: true, value: window });
  Object.defineProperty(globalThis, "document", { configurable: true, value: window.document });
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: window.navigator });
  Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: window.HTMLElement });
  Object.defineProperty(globalThis, "HTMLTextAreaElement", { configurable: true, value: window.HTMLTextAreaElement });
  Object.defineProperty(globalThis, "Event", { configurable: true, value: window.Event });
  Object.defineProperty(globalThis, "InputEvent", { configurable: true, value: window.InputEvent });
  Object.defineProperty(globalThis, "KeyboardEvent", { configurable: true, value: window.KeyboardEvent });
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    value: (callback: FrameRequestCallback) => {
      callback(0);
      return 0;
    },
  });
  Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: () => {} });
  Object.defineProperty(window.navigator, "userAgent", {
    configurable: true,
    value: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 WebKitGTK",
  });
  return window;
}

const muxState: ExtendedState = {
  workspaces: [],
  tabs: {},
  teams: [],
  active_workspace_id: null,
  focused_tab_id: null,
  notifications: [],
  tabStatuses: {},
  toasts: [],
  activeTabIds: {},
  generation: 1,
  pendingCloses: [],
};
const muxStore = createMuxStateStore(() => muxState);

async function mount(tabId: string, optimistic: string[]): Promise<{
  root: Root;
  host: HTMLDivElement;
  composer: HTMLTextAreaElement;
}> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <MuxStateStoreContext.Provider value={muxStore}>
        <ChatComposer
          tabId={tabId}
          tabProvider={null}
          currentPtyId={null}
          pendingPrompt={null}
          attachments={[]}
          addAttachments={() => {}}
          removeAttachment={() => {}}
          clearAttachments={() => {}}
          handlePasteEvent={() => {}}
          onWillSendMessage={() => {}}
          addOptimistic={(message) => {
            optimistic.push(message);
            return "optimistic";
          }}
          updateOptimistic={() => {}}
          observeChatSendStatus={() => {}}
          showComposerError={() => {}}
          chatBackend
        />
      </MuxStateStoreContext.Provider>,
    );
    await Promise.resolve();
  });
  const composer = host.querySelector('[data-testid="chat-composer"]') as HTMLTextAreaElement | null;
  assert(composer, "composer did not mount");
  return { root, host, composer };
}

function setTextareaValue(textarea: HTMLTextAreaElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
  setter?.call(textarea, value);
}

async function main(): Promise<void> {
  const window = setupDom();
  __setInvokeForTest(async (command) => {
    if (command === "delayed_send_list") return [] as never;
    if (command === "chat_send_text") return {
      state: "accepted",
      send_id: "send-1",
      tab_id: "webkitgtk",
      error: null,
      ledger_id: "ledger-1",
      sequence: 1,
      retained_from: 1,
    } as never;
    return {} as never;
  });
  try {
    const webKitOptimistic: string[] = [];
    const webKit = await mount("webkitgtk", webKitOptimistic);
    await act(async () => {
      webKit.composer.dispatchEvent(new InputEvent("beforeinput", {
        bubbles: true,
        cancelable: true,
        data: "x",
        inputType: "insertText",
      }));
      setTextareaValue(webKit.composer, "xx");
      webKit.composer.dispatchEvent(new Event("input", { bubbles: true }));
      await Promise.resolve();
    });
    assert(webKit.composer.value === "x", "WebKitGTK beforeinput/input duplication escaped the controlled DOM");
    await act(async () => {
      webKit.composer.dispatchEvent(new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
      }));
      await Promise.resolve();
    });
    assert(webKitOptimistic.join(",") === "x", "WebKitGTK duplicate input reached React send state");
    await act(async () => webKit.root.unmount());
    webKit.host.remove();

    const chromiumOptimistic: string[] = [];
    const chromium = await mount("chromium", chromiumOptimistic);
    await act(async () => {
      setTextareaValue(chromium.composer, "y");
      chromium.composer.dispatchEvent(new Event("input", { bubbles: true }));
      await Promise.resolve();
    });
    assert(chromium.composer.value === "y", "single Chromium input did not keep one DOM character");
    await act(async () => {
      chromium.composer.dispatchEvent(new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
      }));
      await Promise.resolve();
    });
    assert(chromiumOptimistic.join(",") === "y", "single Chromium input did not keep one React character");
    await act(async () => chromium.root.unmount());
    chromium.host.remove();
    console.log("WebKitGTK composer input ownership checks passed");
  } finally {
    __setInvokeForTest(null);
    await window.happyDOM.abort();
  }
}

void main().then(
  () => process.exit(0),
  (error) => {
    console.error(error);
    process.exit(1);
  },
);
