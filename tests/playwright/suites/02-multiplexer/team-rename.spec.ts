/**
 * WHAT: Team Rename uses a Team manager identity instead of the focused pane.
 * WHY: An unrelated focused pane must never make the rename request unauthorized.
 * ORACLE: The Team name changes in the existing desktop UI without a reload.
 */
import { expect, test } from "../../helpers/fixtures";

test("@smoke Team Rename ignores unrelated focused pane", async ({ api, ui, paneCleanup }) => {
  test.setTimeout(30_000);
  const manager = await paneCleanup.createPane<{ id: string; tab_id: string }>({ title: "pw-team-manager" });
  const unrelated = await paneCleanup.createPane<{ id: string; tab_id: string }>({ title: "pw-team-unrelated" });
  await api.post(`/panes/${manager.id}/role`, { role: "Orchestrator" });
  await api.focusPane(unrelated.id);

  const team = ui.page.locator('[data-testid="team-unit"]').filter({ hasText: "pw-team-manager" });
  await expect(team).toBeVisible({ timeout: 15_000 });
  await team.locator('[data-testid="team-menu-trigger"]').click();
  await ui.page.getByRole("button", { name: "Rename" }).click();
  const name = `pw-team-renamed-${Date.now()}`;
  const input = team.locator('[data-testid="team-name-input"]');
  await input.fill(name);
  await input.press("Enter");
  await expect(team.locator('[data-testid="team-name"]')).toHaveText(name, { timeout: 15_000 });
});
