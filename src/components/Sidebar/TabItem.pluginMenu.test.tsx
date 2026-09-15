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
type PluginTabMenuState = import("../../lib/pluginTabMenu").PluginTabMenuState;

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
    iconSvg: '<svg viewBox="0 0 1 1" fill="currentColor"><path d="M0 0"/></svg>',
    iconColor: "#336699",
  };
  const ITEM_TEST_ID = `tab-menu-plugin-${ITEM.pluginId}-${ITEM.itemId}`;

  const bridge = {
    available: false,
    bindings: {} as Record<string, string>,
    runs: [] as Array<{ pluginId: string; itemId: string; tabId: string }>,
    loads: 0,
    watches: 0,
    releases: 0,
    onChange: null as (() => void) | null,
  };
  // Read counters through a call so an earlier assertion does not narrow later comparisons.
  const counts = () => ({ loads: bridge.loads, watches: bridge.watches, releases: bridge.releases });
  const current = (): PluginTabMenuState => ({ items: [ITEM], receiverTargets: { ...bridge.bindings } });

  menuStore.__setPluginTabMenuForTest(current(), {
    available: () => bridge.available,
    load: async () => {
      bridge.loads += 1;
      return current();
    },
    run: async (item, tabId) => {
      bridge.runs.push({ pluginId: item.pluginId, itemId: item.itemId, tabId });
      bridge.bindings[item.pluginId] = tabId;
      return current();
    },
    watch: async (onChange) => {
      bridge.watches += 1;
      bridge.onChange = onChange;
      return () => {
        bridge.releases += 1;
        bridge.onChange = null;
      };
    },
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

  async function flush() {
    await act(async () => {
      await Promise.resolve();
    });
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
    // Portals append in order, so the newest open menu is the last match.
    const menus = document.querySelectorAll('[role="menu"][aria-label="Tab actions"]');
    const menu = menus[menus.length - 1];
    assert(menu, "menu did not open from the keyboard");
    return menu;
  }

  async function openPlugins(host: Element) {
    const menu = await openMenu(host);
    const plugins = menu.querySelector('[data-testid="tab-menu-plugins"]');
    assert(plugins, "the kebab has no Plugins entry");
    assert(plugins.getAttribute("aria-haspopup") === "menu", "Plugins announces a submenu");
    assert(!menu.querySelector(`[data-testid="${ITEM_TEST_ID}"]`), "plugin items stay out of the top level");
    await act(async () => {
      (plugins as unknown as { click: () => void }).click();
    });
    return menu;
  }

  // Without a host bridge the menu still renders, and nothing is fetched or listened to.
  const local = await mount(plainTab());
  let menu = await openPlugins(local.host);
  let item = menu.querySelector(`[data-testid="${ITEM_TEST_ID}"]`);
  assert(item, "the declared plugin item is missing from the Plugins submenu");
  assert(item.getAttribute("role") === "menuitemcheckbox", "the plugin item exposes a checkbox role");
  assert(item.getAttribute("aria-checked") === "false", "an unbound tab is unchecked");
  assert(item.textContent?.trim() === ITEM.label, "the accessible name is the declared label");
  assert(item.querySelector("svg")?.getAttribute("aria-hidden") === "true", "plugin menu icon is aria-hidden");
  assert(item.querySelector("svg")?.getAttribute("fill") === "currentColor", "plugin menu icon inherits currentColor");
  const iconStyle = item.querySelector("svg")?.parentElement?.getAttribute("style") ?? "";
  assert(iconStyle.includes("#336699") || iconStyle.includes("rgb(51, 102, 153)"), `plugin icon keeps its manifest color: ${iconStyle}`);
  assert(item.getAttribute("aria-label") === null, "the menu item keeps its text label as the accessible name");
  assert(menu.querySelector('[data-testid="tab-menu-telegram-connect"]'), "Telegram sits beside the plugin items");

  await act(async () => {
    (item as unknown as { click: () => void }).click();
  });
  assert(bridge.runs.length === 1, "one click runs the item once");
  assert(
    bridge.runs[0].pluginId === ITEM.pluginId && bridge.runs[0].itemId === ITEM.itemId && bridge.runs[0].tabId === "tab-plain",
    "the click carries the plugin, item and clicked tab id",
  );

  menu = await openPlugins(local.host);
  item = menu.querySelector(`[data-testid="${ITEM_TEST_ID}"]`);
  assert(item?.getAttribute("aria-checked") === "true", "the bound tab renders checked");
  await local.dispose();
  assert(counts().loads === 0 && counts().watches === 0, "no host bridge means no fetch and no listener");
  assert(!menuStore.__pluginTabMenuSyncForTest().listening, "no host bridge leaves nothing listening");

  // With a host bridge every mounted row shares one listener, released by the last unmount.
  bridge.available = true;
  const other = await mount(plainTab({ id: "tab-other", pty_id: "pty-other" }));
  const remote = await mount(plainTab({ id: "tab-remote", remote_peer_id: "peer-1" }));
  await flush();
  assert(counts().watches === 1, "mounted rows share one contributions listener");
  assert(counts().loads >= 1, "the first subscriber loads the menu state");

  const otherMenu = await openPlugins(other.host);
  assert(
    otherMenu.querySelector(`[data-testid="${ITEM_TEST_ID}"]`)?.getAttribute("aria-checked") === "false",
    "only the one bound tab is checked",
  );
  const remoteMenu = await openMenu(remote.host);
  assert(!remoteMenu.querySelector('[data-testid="tab-menu-plugins"]'), "a peer's tab has no Plugins entry");
  assert(!remoteMenu.querySelector('[role="menuitemcheckbox"]'), "a peer's tab offers no local receiver action");

  bridge.bindings[ITEM.pluginId] = "tab-other";
  const loadsBefore = counts().loads;
  bridge.onChange?.();
  await flush();
  assert(counts().loads === loadsBefore + 1, "a contributions event reloads the menu state");
  const reloaded = await openPlugins(other.host);
  assert(
    reloaded.querySelector(`[data-testid="${ITEM_TEST_ID}"]`)?.getAttribute("aria-checked") === "true",
    "the reloaded binding checks its tab",
  );

  await other.dispose();
  assert(counts().releases === 0, "the listener stays while a row is still mounted");
  await remote.dispose();
  assert(counts().releases === 1, "the last unmount releases the listener");
  assert(bridge.onChange === null, "no event handler outlives the last row");
  const sync = menuStore.__pluginTabMenuSyncForTest();
  assert(sync.subscribers === 0 && !sync.listening, "mount and unmount leave no pending listener");

  const again = await mount(plainTab({ id: "tab-again", pty_id: "pty-again" }));
  await flush();
  await again.dispose();
  assert(counts().watches === 2 && counts().releases === 2, "a remount listens again and releases again");

  console.log("TabItem plugin menu: ok");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
