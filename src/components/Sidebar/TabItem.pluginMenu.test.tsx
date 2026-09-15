// Run: npx tsx src/components/Sidebar/TabItem.pluginMenu.test.tsx

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

type Tab = import("../../lib/types").Tab;
type PluginTabMenuItem = import("../../lib/pluginTabMenu").PluginTabMenuItem;

async function main() {
  const [{ act }, { createRoot }, { TabItem }, menuStore] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    import("./TabItem"),
    import("../../lib/pluginTabMenu"),
  ]);

  function assert(condition: unknown, message: string): asserts condition {
    if (!condition) throw new Error(message);
  }

  const ITEM: PluginTabMenuItem = {
    pluginId: "receiver",
    itemId: "receiver-target",
    label: "Receive events here",
    action: "receiverTarget",
  };
  const ITEM_TEST_ID = `tab-menu-plugin-${ITEM.pluginId}-${ITEM.itemId}`;
  const runs: Array<{ pluginId: string; itemId: string; tabId: string }> = [];

  menuStore.__setPluginTabMenuForTest({ items: [ITEM], receiverTargets: {} }, async (item, tabId) => {
    runs.push({ pluginId: item.pluginId, itemId: item.itemId, tabId });
    return { items: [ITEM], receiverTargets: { [item.pluginId]: tabId } };
  });

  function plainTab(overrides: Partial<Tab> = {}): Tab {
    return {
      id: "tab-plain",
      pty_id: "pty-plain",
      tab_type: "Terminal",
      title: "Shell",
      cwd: null,
      protected: false,
      ...overrides,
    };
  }

  async function mount(tab: Tab) {
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
          canClose
          workspaceId="workspace"
          hasAgent={false}
          status={null}
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

  async function openMenu(host: Element) {
    const trigger = host.querySelector('[aria-label="Tab actions"]')?.parentElement;
    assert(trigger, "kebab trigger is missing");
    await act(async () => {
      trigger.dispatchEvent(
        new window.KeyboardEvent("keydown", { key: "Enter", bubbles: true }) as unknown as Event,
      );
    });
    const menu = document.querySelector('[role="menu"][aria-label="Tab actions"]');
    assert(menu, "menu did not open from the keyboard");
    return menu;
  }

  const local = await mount(plainTab());
  let menu = await openMenu(local.host);
  let item = menu.querySelector(`[data-testid="${ITEM_TEST_ID}"]`);
  assert(item, "the declared plugin item is missing from the kebab");
  assert(item.getAttribute("role") === "menuitemcheckbox", "the plugin item exposes a checkbox role");
  assert(item.getAttribute("aria-checked") === "false", "an unbound tab is unchecked");
  assert(item.textContent?.trim() === ITEM.label, "the accessible name is the declared label");

  await act(async () => {
    (item as unknown as { click: () => void }).click();
  });
  assert(runs.length === 1, "one click runs the item once");
  assert(
    runs[0].pluginId === ITEM.pluginId && runs[0].itemId === ITEM.itemId && runs[0].tabId === "tab-plain",
    "the click carries the plugin, item and clicked tab id",
  );

  menu = await openMenu(local.host);
  item = menu.querySelector(`[data-testid="${ITEM_TEST_ID}"]`);
  assert(item?.getAttribute("aria-checked") === "true", "the bound tab renders checked");
  await local.dispose();

  const other = await mount(plainTab({ id: "tab-other", pty_id: "pty-other" }));
  const otherMenu = await openMenu(other.host);
  assert(
    otherMenu.querySelector(`[data-testid="${ITEM_TEST_ID}"]`)?.getAttribute("aria-checked") === "false",
    "only the one bound tab is checked",
  );
  await other.dispose();

  const remote = await mount(plainTab({ id: "tab-remote", remote_peer_id: "peer-1" }));
  const remoteMenu = await openMenu(remote.host);
  assert(!remoteMenu.querySelector('[role="menuitemcheckbox"]'), "a peer's tab offers no local receiver action");
  await remote.dispose();

  console.log("TabItem plugin menu: ok");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
