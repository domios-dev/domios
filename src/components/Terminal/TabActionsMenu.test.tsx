// Run: npx tsx src/components/Terminal/TabActionsMenu.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { TabActionsMenu } from "./TabActionsMenu";
import { setTabPermissionAutoConfirm } from "./tabSessionActions";
import { __setInvokeForTest } from "../../lib/bridge";

type TestFn = () => void | Promise<void>;

const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/", width: 1280, height: 800 });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  for (const key of ["document", "navigator"] as const) {
    Object.defineProperty(globalThis, key, { value: window[key], configurable: true });
  }
  for (const key of [
    "HTMLElement",
    "HTMLButtonElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
    "MouseEvent",
    "KeyboardEvent",
  ] as const) {
    Object.defineProperty(globalThis, key, { value: window[key], configurable: true });
  }
}

type MenuProps = React.ComponentProps<typeof TabActionsMenu>;

function props(overrides: Partial<MenuProps>): MenuProps {
  return {
    sessionId: "session-1",
    canReconnect: true,
    canRestart: true,
    reconnecting: false,
    restarting: false,
    onReconnect: () => {},
    onRestart: () => {},
    onError: () => {},
    ...overrides,
  };
}

async function renderOpen(menuProps: MenuProps): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<TabActionsMenu {...menuProps} />);
  });
  const trigger = host.querySelector<HTMLButtonElement>('[data-testid="tab-actions-trigger"]');
  assert(trigger, "menu trigger rendered");
  await act(async () => {
    trigger.click();
  });
  assert(document.querySelector('[data-testid="tab-actions-menu"]'), "menu opened");
  return { host, root };
}

function autoConfirmItem(): HTMLButtonElement | null {
  return document.querySelector<HTMLButtonElement>('[data-testid="tab-permission-auto-confirm"]');
}

async function cleanup(host: HTMLDivElement, root: Root) {
  await act(async () => root.unmount());
  host.remove();
}

test("a recognizing provider gets a checkbox item that reports and requests its state", async () => {
  const requested: boolean[] = [];
  const onToggle = (enabled: boolean) => requested.push(enabled);
  const { host, root } = await renderOpen(
    props({ permissionAutoConfirm: false, onTogglePermissionAutoConfirm: onToggle }),
  );
  const item = autoConfirmItem();
  assert(item, "auto-confirm item renders inside the menu");
  assert(item.closest('[role="menu"]'), "item lives in the tab actions menu");
  assert(item.getAttribute("role") === "menuitemcheckbox", "item is a menuitemcheckbox");
  assert(item.getAttribute("aria-checked") === "false", "item reports the persisted opt-out");
  const describedBy = item.getAttribute("aria-describedby");
  assert(describedBy && document.getElementById(describedBy)?.textContent, "item has an explanation line");

  await act(async () => {
    item.click();
  });
  assert(requested.length === 1 && requested[0] === true, `expected one request to enable, got ${JSON.stringify(requested)}`);
  assert(autoConfirmItem(), "toggling keeps the menu open");

  await act(async () => {
    root.render(<TabActionsMenu {...props({ permissionAutoConfirm: true, onTogglePermissionAutoConfirm: onToggle })} />);
  });
  const enabled = autoConfirmItem();
  assert(enabled?.getAttribute("aria-checked") === "true", "item reflects the persisted opt-in");
  await act(async () => {
    enabled.click();
  });
  assert(
    JSON.stringify(requested) === JSON.stringify([true, false]),
    `an enabled item requests disabling, got ${JSON.stringify(requested)}`,
  );
  await cleanup(host, root);
});

test("arrow keys reach the checkbox item in menu order", async () => {
  const { host, root } = await renderOpen(
    props({ permissionAutoConfirm: false, onTogglePermissionAutoConfirm: () => {} }),
  );
  const menu = document.querySelector('[data-testid="tab-actions-menu"]');
  assert(menu, "menu present");
  const focusables = Array.from(
    menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled), [role="menuitemcheckbox"]:not(:disabled)'),
  );
  assert(focusables[focusables.length - 1] === autoConfirmItem(), "checkbox item is last in focus order");
  focusables[0].focus();
  await act(async () => {
    focusables[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
  });
  assert(document.activeElement === autoConfirmItem(), "ArrowUp from the first item wraps to the checkbox item");
  await cleanup(host, root);
});

test("a provider without the capability shows no auto-confirm item", async () => {
  const { host, root } = await renderOpen(props({ permissionAutoConfirm: null, onTogglePermissionAutoConfirm: () => {} }));
  assert(autoConfirmItem() === null, "no item without the capability");
  assert(document.querySelector('[role="menuitemcheckbox"]') === null, "no checkbox items at all");
  assert(document.querySelector('[data-testid="tab-restart-session"]'), "existing items still render");
  await cleanup(host, root);
});

test("the toggle request goes through the persisted setting command", async () => {
  const calls: Array<[string, unknown]> = [];
  __setInvokeForTest((async (command: string, args?: unknown) => {
    calls.push([command, args]);
    return true;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  try {
    await setTabPermissionAutoConfirm("tab-7", true);
    await setTabPermissionAutoConfirm("tab-7", false);
  } finally {
    __setInvokeForTest(null);
  }
  assert(calls.length === 2, `expected two calls, got ${calls.length}`);
  assert(calls.every(([command]) => command === "mux_set_permission_auto_confirm"), "uses the setting command");
  assert(JSON.stringify(calls.map(([, args]) => args)) === JSON.stringify([
    { tabId: "tab-7", enabled: true },
    { tabId: "tab-7", enabled: false },
  ]), `passes tab and state through: ${JSON.stringify(calls)}`);
});

async function main() {
  setupDom();
  const started = performance.now();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  const elapsedMs = performance.now() - started;
  console.log(`TabActionsMenu tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
