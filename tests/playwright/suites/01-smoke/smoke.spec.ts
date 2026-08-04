/**
 * WHAT: smoke covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 01-smoke.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";

test("@smoke /api/health is ok", async ({ api }) => {
  const health = await api.health();
  expect(health.status).toBe("ok");
  expect(health.version).toMatch(/^\d+\.\d+\.\d+/);
});

test("@smoke the real app window renders the shell UI", async ({ ui }) => {
  const { page } = ui;
  // The dev console is a Tauri WebView2 window serving the Vite app.
  await expect(page.locator("#root")).toBeAttached();
  // Non-empty render: the shell mounts real content, not a blank root.
  const childCount = await page.locator("#root > *").count();
  expect(childCount).toBeGreaterThan(0);
  // instead — either proves the shell.
  await expect(
    page.locator('[data-testid="terminal-tab"], [data-testid="chat-composer"]').first(),
  ).toBeVisible();
  // Native and the web mirror mount the same titlebar strip (parity decision A).
  await expect(page.locator('[data-testid="titlebar"]')).toBeVisible();
});

test("@smoke default workspace and at least one tab exist", async ({ api }) => {
  const workspaces = await api.workspaces();
  expect(workspaces.length).toBeGreaterThan(0);
  expect(workspaces.some((w) => w.is_active)).toBe(true);

  const tabs = await api.tabs();
  expect(tabs.length).toBeGreaterThan(0);
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
