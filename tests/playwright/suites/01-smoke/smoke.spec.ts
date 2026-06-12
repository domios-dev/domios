/**
 * 01-smoke — proves the harness is real: the dev console is running, its
 * API answers, and Playwright is driving the actual Tauri WebView2 window.
 * All tests @smoke.
 */
import { expect, test } from "../../helpers/fixtures";

test("@smoke /api/health is ok with a build hash", async ({ api }) => {
  const health = await api.health();
  expect(health.status).toBe("ok");
  expect(health.build).toMatch(/^[0-9a-f]{6,40}$/);
  expect(health.version).toMatch(/^\d+\.\d+\.\d+/);
});

test("@smoke the real app window renders the shell UI", async ({ ui }) => {
  const { page } = ui;
  // The dev console is a Tauri WebView2 window serving the Vite app.
  await expect(page.locator("#root")).toBeAttached();
  // Non-empty render: the shell mounts real content, not a blank root.
  const childCount = await page.locator("#root > *").count();
  expect(childCount).toBeGreaterThan(0);
  // A terminal surface exists (xterm.js mounts .xterm containers); a pane
  // left in chat-view shows the composer instead — either proves the shell.
  await expect(
    page.locator('.xterm:visible, [data-testid="chat-composer"]').first(),
  ).toBeVisible();
  // Desktop chrome is present — the web mirror strips it (see 12-web),
  // so this guards the inverse: Tauri must keep its titlebar.
  await expect(page.locator('[data-testid="titlebar"]')).toBeVisible();
});

test("@smoke default workspace and at least one pane exist", async ({ api }) => {
  const workspaces = await api.workspaces();
  expect(workspaces.length).toBeGreaterThan(0);
  expect(workspaces.some((w) => w.is_active)).toBe(true);

  const panes = await api.panes();
  expect(panes.length).toBeGreaterThan(0);
});

test("@smoke system agent providers are registered", async ({ api }) => {
  const providers = await api.providers();
  const ids = providers.map((p) => p.id);
  for (const expected of ["claude", "codex", "opencode"]) {
    expect(ids, `provider ${expected} missing — got ${ids.join(", ")}`).toContain(expected);
  }
});

test("@smoke UI agrees with API about the active workspace", async ({ api, ui }) => {
  const workspaces = await api.workspaces();
  const active = workspaces.find((w) => w.is_active);
  expect(active).toBeDefined();
  await expect(
    ui.page.locator(`[data-testid="workspace-item"][data-workspace-id="${active!.id}"]`),
  ).toBeVisible();
});
