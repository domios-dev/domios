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
Object.defineProperty(globalThis, "requestAnimationFrame", {
  value: (cb: (t: number) => void) => window.setTimeout(() => cb(0), 0),
  configurable: true,
});
Object.defineProperty(globalThis, "cancelAnimationFrame", {
  value: (id: number) => window.clearTimeout(id),
  configurable: true,
});
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type ModelInfo = import("../../lib/types").ModelInfo;
type ProviderDescriptor = import("../../lib/types").ProviderDescriptor;
type Tab = import("../../lib/types").Tab;
type TabStatus = import("../../lib/types").TabStatus;

async function main() {
  const [{ act }, { createRoot }, { TabItem }, { setAgentDescriptors }, { __setTabModelApiForTest }] =
    await Promise.all([
      import("react"),
      import("react-dom/client"),
      import("./TabItem"),
      import("../../lib/agentDescriptorCache"),
      import("./tabModelApi"),
    ]);

  function assert(condition: unknown, message: string): asserts condition {
    if (!condition) throw new Error(message);
  }

  const MODELS: ModelInfo[] = [
    { id: "fixture-small", display_name: "Fixture Small" },
    { id: "fixture-large", display_name: "Fixture Large" },
  ];
  const SESSION_MODEL = MODELS[1];

  function descriptor(overrides: Partial<ProviderDescriptor> = {}): ProviderDescriptor {
    return {
      schema_version: 1,
      id: "fixture-provider",
      display_name: "Fixture Provider",
      transport: "pty",
      system_prompt_delivery: "external",
      available_models: MODELS,
      settings_schema: [],
      notifications_schema: [],
      supports_lazy_skills: false,
      has_hooks: false,
      model_switch_format: "/switch {model}",
      ...overrides,
    } as ProviderDescriptor;
  }

  function generalAgentTab(overrides: Partial<Tab> = {}): Tab {
    return {
      id: "tab-main",
      pty_id: "pty-main",
      tab_type: "Terminal",
      title: "Main",
      cwd: null,
      protected: true,
      agent_type: "fixture-provider",
      agent_session_model: SESSION_MODEL.id,
      ...overrides,
    };
  }

  async function mount(tab: Tab, status: TabStatus | null = null) {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host as unknown as HTMLElement);
    await act(async () => {
      root.render(
        <TabItem
          tab={tab}
          index={0}
          isActive={false}
          onClick={() => undefined}
          onClose={() => undefined}
          canClose={false}
          workspaceId="workspace"
          hasAgent
          status={status}
        />,
      );
    });
    return {
      host,
      async dispose() {
        await act(async () => root.unmount());
        host.remove();
      },
    };
  }

  /** Opens the kebab with the keyboard — the route a keyboard user takes. */
  async function openMenu(host: Element) {
    const trigger = host.querySelector('[aria-label="Tab actions"]')?.parentElement;
    assert(trigger, "kebab trigger is missing");
    await act(async () => {
      trigger.dispatchEvent(
        new window.KeyboardEvent("keydown", { key: "Enter", bubbles: true }) as unknown as Event,
      );
    });
    const menu = document.querySelector('[role="menu"]');
    assert(menu, "menu did not open from the keyboard");
    return menu;
  }

  async function click(element: Element | null | undefined, what: string) {
    assert(element, `${what} is missing`);
    await act(async () => {
      (element as unknown as { click: () => void }).click();
    });
  }

  async function openModelPicker(host: Element) {
    const menu = await openMenu(host);
    const entry = menu.querySelector('[data-testid="tab-menu-model"]');
    assert(entry, "the row menu offers no model entry");
    await click(entry, "model entry");
    return entry;
  }

  // The protected General Agent row offers the entry, named from the descriptor.
  {
    setAgentDescriptors([descriptor()]);
    const view = await mount(generalAgentTab());
    const menu = await openMenu(view.host);
    const entry = menu.querySelector('[data-testid="tab-menu-model"]');
    assert(entry, "the protected General Agent row offers no model entry");
    assert(
      entry.textContent?.includes(SESSION_MODEL.display_name),
      `model entry is not named from the descriptor: ${entry.textContent}`,
    );
    assert(
      entry.getAttribute("aria-haspopup") === "menu",
      "model entry does not announce that it opens a picker",
    );
    assert(
      entry.tagName === "BUTTON" && !(entry as unknown as { disabled: boolean }).disabled,
      "model entry is not an operable control",
    );
    await view.dispose();
  }

  // A provider that publishes no in-agent switch command gets no entry — and the
  // rest of the chain is untouched.
  {
    setAgentDescriptors([descriptor({ model_switch_format: null })]);
    const view = await mount(generalAgentTab());
    const menu = await openMenu(view.host);
    assert(
      !menu.querySelector('[data-testid="tab-menu-model"]'),
      "model entry appeared for a provider that publishes no in-agent switch command",
    );
    assert(
      menu.querySelector('[data-testid="tab-menu-restart-session"]'),
      "the rest of the menu chain disappeared with the model entry",
    );
    await view.dispose();
  }

  // The picker exposes one choice per published model and marks the current one.
  {
    setAgentDescriptors([descriptor()]);
    const view = await mount(generalAgentTab());
    await openModelPicker(view.host);
    const options = document.querySelectorAll('[data-testid^="tab-menu-model-"]');
    assert(
      options.length === MODELS.length,
      `picker listed ${options.length} models, descriptor published ${MODELS.length}`,
    );
    for (const model of MODELS) {
      const option = document.querySelector(`[data-testid="tab-menu-model-${model.id}"]`);
      assert(option, `model ${model.id} is missing from the picker`);
      assert(
        option.getAttribute("role") === "menuitemradio",
        `model ${model.id} is not exposed as one choice in a set`,
      );
      assert(
        option.getAttribute("aria-checked") === String(model.id === SESSION_MODEL.id),
        `model ${model.id} carries the wrong selected state: ${option.getAttribute("aria-checked")}`,
      );
      assert(
        option.textContent?.includes(model.display_name),
        `model ${model.id} is not named from the descriptor`,
      );
      assert(option.tagName === "BUTTON", `model ${model.id} is not keyboard-operable`);
    }
    await view.dispose();
  }

  // Selecting a model switches with the id the fixture supplied.
  {
    const calls: Array<[string, string]> = [];
    __setTabModelApiForTest({
      switchModel: async (ptyId, payload) => {
        calls.push([ptyId, payload]);
      },
    });
    try {
      setAgentDescriptors([descriptor()]);
      const view = await mount(generalAgentTab({ pty_id: "pty-fixture" }));
      await openModelPicker(view.host);
      const target = MODELS[0];
      await click(
        document.querySelector(`[data-testid="tab-menu-model-${target.id}"]`),
        `option ${target.id}`,
      );
      assert(calls.length === 1, `expected one switch, got ${calls.length}`);
      assert(calls[0][0] === "pty-fixture", `switch went to ${calls[0][0]}`);
      assert(
        calls[0][1] === `/switch ${target.id}\r`,
        `switch payload did not round-trip the supplied id: ${JSON.stringify(calls[0][1])}`,
      );
      await view.dispose();
    } finally {
      __setTabModelApiForTest(null);
    }
  }

  // An in-flight turn is not interrupted, and the user is told why.
  {
    setAgentDescriptors([descriptor()]);
    const view = await mount(generalAgentTab(), "Working");
    const menu = await openMenu(view.host);
    const entry = menu.querySelector('[data-testid="tab-menu-model"]');
    assert(entry, "model entry vanished while the agent was working");
    assert(
      (entry as unknown as { disabled: boolean }).disabled,
      "model entry stayed active while a turn was in flight",
    );
    assert(
      (entry.getAttribute("title") ?? "").length > 0,
      "the blocked model entry gives the user no reason",
    );
    await click(entry, "blocked model entry");
    assert(
      !document.querySelector('[data-testid^="tab-menu-model-"]'),
      "the blocked entry still opened the picker",
    );
    await view.dispose();
  }

  console.log("TabItem model menu: ok");
}

void main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
