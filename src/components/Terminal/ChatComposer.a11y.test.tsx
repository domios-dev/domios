// Read DOM attributes and class names because this harness does not compile Tailwind CSS.
import "./ChatComposer.composition.harness";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];

function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function emptyMuxState(): ExtendedState {
  return {
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
}

interface Harness {
  el: HTMLTextAreaElement;
  shell: HTMLElement;
  root: Root;
  host: HTMLDivElement;
}

async function flush(): Promise<void> {
  await act(async () => {
    await new Promise<void>((resolve) => setImmediate(resolve));
  });
}

async function renderComposer(tabId: string): Promise<Harness> {
  __setInvokeForTest((async (command: string) => {
    if (command === "list_agent_providers" || command === "delayed_send_list") return [];
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  __setListenForTest((async () => () => {}) as any);

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const store = createMuxStateStore(emptyMuxState);
  await act(async () => {
    root.render(
      <MuxStateStoreContext.Provider value={store}>
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
          addOptimistic={() => "optimistic-1"}
          updateOptimistic={() => {}}
          observeChatSendStatus={() => {}}
          showComposerError={() => {}}
        />
      </MuxStateStoreContext.Provider>,
    );
  });
  await flush();
  const el = host.querySelector('[data-testid="chat-composer"]');
  assert(el instanceof window.HTMLTextAreaElement, "composer textarea rendered");
  const shell = host.querySelector('[data-testid="composer-input-shell"]');
  assert(shell instanceof window.HTMLElement, "composer shell rendered");
  return { el, shell, root, host };
}

async function teardown(h: Harness): Promise<void> {
  await act(async () => h.root.unmount());
  h.host.remove();
  __setInvokeForTest(null);
  __setListenForTest(null);
}

function setDomValue(el: HTMLTextAreaElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")?.set;
  assert(setter, "textarea value setter available");
  setter.call(el, value);
}

async function fire(el: HTMLTextAreaElement, event: Event): Promise<void> {
  await act(async () => {
    el.dispatchEvent(event);
  });
}

function inputEvent(): Event {
  const event = new window.Event("input", { bubbles: true });
  Object.defineProperty(event, "isComposing", { value: false });
  Object.defineProperty(event, "inputType", { value: "insertText" });
  return event;
}

function enterKeydown(): Event {
  const event = new window.KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  Object.defineProperty(event, "isComposing", { value: false });
  Object.defineProperty(event, "keyCode", { value: 13 });
  return event;
}

async function type(h: Harness, value: string): Promise<void> {
  setDomValue(h.el, value);
  await fire(h.el, inputEvent());
}

test("composer textarea carries an accessible name", async () => {
  const h = await renderComposer("tab-a11y-name");
  assert(h.el.getAttribute("aria-label") === "Message composer", "textarea must expose an accessible name");
  await teardown(h);
});

test("composer shell declares a visible keyboard-focus style", async () => {
  const h = await renderComposer("tab-a11y-focus");
  assert(
    h.shell.className.includes("focus-within:ring"),
    "the shell wrapping the borderless textarea must show a focus ring — it had none before ct#330",
  );
  await teardown(h);
});

test("a rejected /model query sets aria-invalid, and editing clears it", async () => {
  const h = await renderComposer("tab-a11y-invalid");
  assert(
    h.el.getAttribute("aria-invalid") === "false",
    "an untouched composer must not announce itself as invalid",
  );

  await type(h, "/model this-model-does-not-exist");
  await fire(h.el, enterKeydown());
  await flush();

  assert(
    h.el.getAttribute("aria-invalid") === "true",
    "a rejected /model query must be announced via aria-invalid",
  );

  await type(h, "/model this-model-does-not-exist ");
  assert(
    h.el.getAttribute("aria-invalid") === "false",
    "editing the field again must clear the invalid announcement",
  );

  await teardown(h);
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed++;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
    }
  }
  if (failed > 0) process.exit(1);
  console.log(`\n✓ all ${tests.length} ChatComposer a11y tests green`);
}

void main();
