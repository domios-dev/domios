import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import { TabItem } from "./TabItem";
import type { Tab } from "../../lib/types";
import type { ShellPurpose } from "../../lib/shellCommandState";
import { act, createRoot, createSuite, setupDom } from "./SidebarWorkspaceTabs.testkit";

setupDom();
const { test, run } = createSuite("TabItem shell wiring");
const shell: Tab = {
  id: "utility", pty_id: "pty", tab_type: "Terminal", title: "Utility", cwd: null,
  description: "Compiling the application",
};

function row(tab: Tab, onClose = () => {}) {
  return <TabItem tab={tab} index={0} isActive={false} onClick={() => {}}
    onClose={onClose} canClose workspaceId="workspace" />;
}

test("shell purposes retain title and description, no status, and an actions button for close", () => {
  for (const purpose of ["build", "dev", "qa", "shell"] as ShellPurpose[]) {
    for (const activity of ["unknown", "running", "idle"] as const) {
      const host = document.createElement("div");
      host.innerHTML = renderToStaticMarkup(row({
        ...shell, purpose, shell_command_state: { activity, last_exit_code: null },
      }));
      const label = purpose === "shell" ? "shell" : `${purpose} shell`;
      assert.ok(host.querySelector(`button[aria-label="Open tab ${label}, Utility"]`));
      assert.ok(host.querySelector(`[role="img"][aria-label="${label}"]`));
      assert.equal(host.querySelector('[data-testid="tab-title"]')?.textContent, shell.title);
      assert.equal(host.querySelector('[data-testid="tab-description-subtitle"]')?.textContent, shell.description);
      assert.equal(host.querySelector('[role="status"]'), null);
      assert.ok(host.querySelector('button[aria-label="Tab actions"]'));
      assert.equal(Boolean(host.querySelector('[role="img"][aria-label="Utility working"]')), activity === "running");
    }
  }
});

test("custom utility icon reaches the row with its description", () => {
  const host = document.createElement("div");
  host.innerHTML = renderToStaticMarkup(row({ ...shell, purpose: "qa", icon: "flask-conical" }));
  assert.ok(host.querySelector('[role="img"][aria-label="flask-conical, qa shell"]'));
  assert.equal(host.querySelector('[data-testid="tab-description-subtitle"]')?.textContent, shell.description);
});

test("shell context menu retains rename and close actions", async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  let closed = 0;
  try {
    await act(async () => root.render(row(shell, () => { closed += 1; })));
    const target = host.querySelector('[role="group"]');
    assert.ok(target);
    const open = async () => act(async () => {
      target.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    });
    await open();
    const rename = document.querySelector<HTMLButtonElement>('[role="menuitem"][data-testid="tab-menu-rename"]');
    assert.ok(rename);
    assert.ok(document.querySelector('[role="menuitem"][data-testid="tab-menu-close"]'));
    await act(async () => rename.click());
    const input = host.querySelector<HTMLInputElement>('[aria-label="Rename tab Utility"]');
    assert.ok(input);
    await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    await open();
    const close = document.querySelector<HTMLButtonElement>('[role="menuitem"][data-testid="tab-menu-close"]');
    assert.ok(close);
    await act(async () => close.click());
    assert.equal(closed, 1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

void run();
