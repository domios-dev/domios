/**
 * WHAT: Renaming a Team persists while an unrelated tab owns focus.
 * WHY: Team actions must target the selected Team rather than the focused tab.
 * ORACLE: The Team heading changes and remains changed after reloading the UI.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";

interface TeamPlan {
  plan_id: string;
  team_id: string;
}

test("@smoke Team Rename persists with an unrelated focused tab", async ({ api, ui, tabCleanup }) => {
  test.setTimeout(30_000);
  const projectPath = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-team-rename-")));
  await api.createWorkspace("pw Team Rename", projectPath);
  const workspace = (await api.workspaces()).find((entry) => entry.cwd === projectPath);
  if (!workspace) throw new Error("Team Rename fixture workspace was not created");
  const manager = await tabCleanup.createTab<{ id: string }>({ workspace_id: workspace.id, cwd: projectPath, title: "pw Team Rename manager" });
  const unrelated = await tabCleanup.createTab<{ id: string }>({ workspace_id: workspace.id, cwd: projectPath, title: "pw Team Rename unrelated" });
  await api.post(`/tabs/${manager.id}/role`, { role: "Orchestrator" });
  const created = await api.post<TeamPlan>("/plans", {
    title: "pw Team Rename plan",
    project_path: projectPath,
    owner: { tab_id: manager.id, session_id: null, title: "pw Team Rename manager" },
    kinds: "implement:#38bdf8",
  });
  await api.focusWorkspace(workspace.id);
  await api.focusTab(unrelated.id);

  try {
    const team = ui.page.locator(`[data-testid="team-unit"][data-team-id="${created.team_id}"]`);
    await expect(team).toBeVisible({ timeout: 15_000 });
    const urlBeforeRename = ui.page.url();
    await team.locator('[data-testid="team-menu-trigger"]').click();
    await ui.page.getByTestId("team-menu").getByRole("button", { name: "Rename", exact: true }).click();
    const name = `pw-team-renamed-${Date.now()}`;
    const input = team.locator('[data-testid="team-name-input"]');
    await input.fill(name);
    await input.press("Enter");
    await expect(team.locator('[data-testid="team-name"]')).toHaveText(name, { timeout: 15_000 });
    expect(ui.page.url(), "rename updates mux state without navigation or reload").toBe(urlBeforeRename);
    const snapshot = await api.get<unknown>("/mux/state");
    expect(JSON.stringify(snapshot)).toContain(created.team_id);
    expect(JSON.stringify(snapshot)).toContain(name);
  } finally {
    await api.deletePlan(created.plan_id).catch(() => {});
    await api.closeWorkspace(workspace.id).catch(() => {});
  }
});
