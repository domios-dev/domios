/**
 * 01-smoke — proves the harness is real: the dev console is running, its
 * API answers, and Playwright is driving the actual Tauri WebView2 window.
 * All tests @smoke.
 */
import { expect, test } from "@playwright/test";
import { ApiClient } from "../../helpers/api-client";
import { connectUi, type UiSession } from "../../helpers/dev-console";

const api = new ApiClient();
let ui: UiSession;

test.beforeAll(async () => {
  ui = await connectUi();
});

test.afterAll(async () => {
  // Disconnect only — never close the real app window from a smoke test.
  await ui.browser.close();
});

test("@smoke /api/health is ok with a build hash", async () => {
  const health = await api.health();
  expect(health.status).toBe("ok");
  expect(health.build).toMatch(/^[0-9a-f]{6,40}$/);
  expect(health.version).toMatch(/^\d+\.\d+\.\d+/);
});

test("@smoke the real app window renders the shell UI", async () => {
  const { page } = ui;
  // The dev console is a Tauri WebView2 window serving the Vite app.
  await expect(page.locator("#root")).toBeAttached();
  // Non-empty render: the shell mounts real content, not a blank root.
  const childCount = await page.locator("#root > *").count();
  expect(childCount).toBeGreaterThan(0);
  // A terminal surface exists (xterm.js mounts .xterm containers).
  await expect(page.locator(".xterm").first()).toBeVisible();
});

test("@smoke default workspace and at least one pane exist", async () => {
  const workspaces = await api.workspaces();
  expect(workspaces.length).toBeGreaterThan(0);
  expect(workspaces.some((w) => w.is_active)).toBe(true);

  const panes = await api.panes();
  expect(panes.length).toBeGreaterThan(0);
});

test("@smoke system agent providers are registered", async () => {
  const providers = await api.providers();
  const ids = providers.map((p) => p.id);
  for (const expected of ["claude", "codex", "opencode"]) {
    expect(ids, `provider ${expected} missing — got ${ids.join(", ")}`).toContain(expected);
  }
});

test("@smoke UI agrees with API about the active workspace", async () => {
  const workspaces = await api.workspaces();
  const active = workspaces.find((w) => w.is_active);
  expect(active).toBeDefined();
  // The workspace name is rendered somewhere in the shell (sidebar/titlebar).
  await expect(ui.page.getByText(active!.name).first()).toBeVisible();
});
