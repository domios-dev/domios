/**
 * WHAT: live-provider reality of the picker pair spawn — the orchestrator's
 *       first act creates and claims the pair's plan (lazy, LLM-driven).
 * WHY: golden covers picker plumbing on the stub transport; plan creation is
 *      real agent behavior and stays under @live on its own cadence.
 * ORACLE: /api/plans contains a plan titled with the workspace name shortly
 *      after the pair materializes.
 */
import { expect, test } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import type { ApiClient } from "../../helpers/api-client";

async function createTestWorkspace(api: ApiClient): Promise<{ id: string; name: string }> {
  const name = `pw-live-pair-${Date.now()}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((w) => w.name === name);
  if (!ws) throw new Error(`workspace ${name} missing after create`);
  await api.focusWorkspace(ws.id);
  return { id: ws.id, name };
}

test("@live pair spawn: the live orchestrator creates and claims the pair's plan", async ({
  api,
  ui,
}) => {
  test.setTimeout(420_000);
  const ws = await createTestWorkspace(api);
  const page = ui.page;

  const item = page.locator(`[data-testid="workspace-item"][data-workspace-id="${ws.id}"]`);
  await expect(item).toBeVisible({ timeout: 15_000 });
  const expand = item.locator('button[title="Expand"]');
  if (await expand.isVisible().catch(() => false)) await expand.click();
  await page
    .locator(`[data-testid="workspace-group"][data-workspace-id="${ws.id}"]`)
    .getByText("New session", { exact: true })
    .click();
  await expect(page.locator('[data-testid="new-session-picker"]')).toBeVisible({ timeout: 10_000 });
  await page.locator('[data-testid="picker-mode-orchestration"]').click();
  await page.locator('[data-testid="picker-confirm"]').click();
  await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });

  const plan = await pollUntil(
    async () => {
      const plans = await api.plans().catch(() => []);
      return plans.find((p) => (p.title ?? "").includes(ws.name)) ?? null;
    },
    240_000,
    3_000,
    "live orchestrator creates the pair plan",
  );
  expect(plan, "the pair's plan exists").toBeTruthy();
  expect(plan!.title, "plan title includes the workspace name").toContain(ws.name);
  expect(plan!.title.trim(), "plan title is not the bare 'Orchestration'").not.toBe("Orchestration");
});
