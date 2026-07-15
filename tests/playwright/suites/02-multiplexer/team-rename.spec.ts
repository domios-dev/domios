import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";

interface TeamPlan {
  plan_id: string;
  team_id: string;
}

test("@smoke Team Rename persists with an unrelated focused pane", async ({ api, ui, paneCleanup }) => {
  test.setTimeout(30_000);
  const projectPath = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-team-rename-")));
  await api.createWorkspace("pw Team Rename", projectPath);
  const workspace = (await api.workspaces()).find((entry) => entry.cwd === projectPath);
  if (!workspace) throw new Error("Team Rename fixture workspace was not created");
  const manager = await paneCleanup.createPane<{ id: string }>({ workspace_id: workspace.id, cwd: projectPath, title: "pw Team Rename manager" });
  const unrelated = await paneCleanup.createPane<{ id: string }>({ workspace_id: workspace.id, cwd: projectPath, title: "pw Team Rename unrelated" });
  await api.post(`/panes/${manager.id}/role`, { role: "Orchestrator" });
  const created = await api.post<TeamPlan>("/plans", {
    title: "pw Team Rename plan",
    project_path: projectPath,
    owner: { pane_id: manager.id, session_id: null, title: "pw Team Rename manager" },
    kinds: "implement:#38bdf8",
  });
  await api.focusWorkspace(workspace.id);
  await api.focusPane(unrelated.id);

  try {
    const team = ui.page.locator(`[data-testid="team-unit"][data-team-id="${created.team_id}"]`);
    await expect(team).toBeVisible({ timeout: 15_000 });
    const urlBeforeRename = ui.page.url();
    await team.locator('[data-testid="team-menu-trigger"]').click();
    await ui.page.getByRole("button", { name: "Rename" }).click();
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

test("@smoke Team Rename shows a forced 403 without local replacement", async ({ api, ui, paneCleanup }) => {
  test.setTimeout(30_000);
  const projectPath = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-team-rename-403-")));
  await api.createWorkspace("pw Team Rename 403", projectPath);
  const workspace = (await api.workspaces()).find((entry) => entry.cwd === projectPath);
  if (!workspace) throw new Error("Team Rename 403 fixture workspace was not created");
  const manager = await paneCleanup.createPane<{ id: string }>({ workspace_id: workspace.id, cwd: projectPath, title: "pw Team Rename 403 manager" });
  await api.post(`/panes/${manager.id}/role`, { role: "Orchestrator" });
  const created = await api.post<TeamPlan>("/plans", {
    title: "pw Team Rename 403 plan",
    project_path: projectPath,
    owner: { pane_id: manager.id, session_id: null, title: "pw Team Rename 403 manager" },
    kinds: "implement:#38bdf8",
  });
  await api.focusWorkspace(workspace.id);

  try {
    const team = ui.page.locator(`[data-testid="team-unit"][data-team-id="${created.team_id}"]`);
    await expect(team).toBeVisible({ timeout: 15_000 });
    const urlBeforeRename = ui.page.url();
    await ui.page.evaluate(() => {
      const internals = window.__TAURI_INTERNALS__;
      const invoke = internals.invoke.bind(internals);
      internals.invoke = (command: string, args?: Record<string, unknown>) => command === "mux_rename_team"
        ? Promise.reject(new Error("Team manager permission denied (403)"))
        : invoke(command, args);
    });
    const originalName = await team.locator('[data-testid="team-name"]').textContent();
    await team.locator('[data-testid="team-menu-trigger"]').click();
    await ui.page.getByRole("button", { name: "Rename" }).click();
    const input = team.locator('[data-testid="team-name-input"]');
    await input.fill("Rejected Team Rename");
    await input.press("Enter");
    await expect(team.locator('[data-testid="team-rename-error"]')).toContainText("403");
    await expect(input).toBeVisible();
    await expect(input).toHaveValue("Rejected Team Rename");
    expect(ui.page.url(), "rejection does not reload or replace the local page").toBe(urlBeforeRename);
    await input.press("Escape");
    await expect(team.locator('[data-testid="team-name"]')).toHaveText(originalName ?? "Team");
  } finally {
    await api.deletePlan(created.plan_id).catch(() => {});
    await api.closeWorkspace(workspace.id).catch(() => {});
  }
});
