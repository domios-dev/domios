/**
 * WHAT: Renaming a Team persists while an unrelated tab owns focus.
 * WHY: Team actions must target the selected Team rather than the focused tab.
 * ORACLE: The Team heading changes and the new name is in mux state.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import { spawnBoundAgent } from "../../helpers/session-binding";
import { isStubTransportActive } from "../../helpers/stub-gate";

interface MuxTeam {
  id: string;
  workspace_id: string;
}

test("@smoke Team Rename persists with an unrelated focused tab", async ({ api, ui, tabCleanup }) => {
  test.skip(
    !(await isStubTransportActive(api)),
    "the Orchestrator role needs a verified session binding, which only a spawned agent reports",
  );
  test.setTimeout(240_000);
  const projectPath = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-team-rename-")));
  await api.createWorkspace("pw Team Rename", projectPath);
  const workspace = (await api.workspaces()).find((entry) => entry.cwd === projectPath);
  if (!workspace) throw new Error("Team Rename fixture workspace was not created");
  const unrelated = await tabCleanup.createTab<{ id: string }>({ workspace_id: workspace.id, cwd: projectPath, title: "pw Team Rename unrelated" });

  // A Team is minted by the TRANSITION into an anchoring role, not by spawning
  // one: a spawn that already carries the role has nothing to transition. The
  const managerId = await spawnBoundAgent(api, {
    workspace_id: workspace.id,
    cwd: projectPath,
    title: "pw Team Rename manager",
  });
  await api.post(`/tabs/${managerId}/make-orchestrator`, {});
  const teamId = await pollUntil(
    async () => {
      const teams = (await api.get<{ teams?: MuxTeam[] }>("/mux/state")).teams ?? [];
      return teams.find((entry) => entry.workspace_id === workspace.id)?.id;
    },
    30_000,
    250,
    "a Team minted in the fixture workspace",
  );

  await api.focusWorkspace(workspace.id);
  await api.focusTab(unrelated.id);

  try {
    const team = ui.page.locator(`[data-testid="team-unit"][data-team-id="${teamId}"]`);
    await expect(team).toBeVisible({ timeout: 15_000 });
    const urlBeforeRename = ui.page.url();
    // The kebab is opacity:0 / pointer-events:none until its header is hovered,
    // so a bare click hit-tests onto the header and never becomes actionable.
    await team.locator(".sidebar-team-header").first().hover();
    await team.locator('[data-testid="team-menu-trigger"]').click();
    // The entries carry role="menuitem"; an explicit role wins over the tag, so
    // getByRole("button") never matches them.
    await ui.page.getByTestId("team-menu").getByRole("menuitem", { name: "Rename", exact: true }).click();
    const name = `pw-team-renamed-${Date.now()}`;
    const input = team.locator('[data-testid="team-name-input"]');
    await input.fill(name);
    await input.press("Enter");
    await expect(team.locator('[data-testid="team-name"]')).toHaveText(name, { timeout: 15_000 });
    expect(ui.page.url(), "rename updates mux state without navigation or reload").toBe(urlBeforeRename);
    const snapshot = await api.get<unknown>("/mux/state");
    expect(JSON.stringify(snapshot)).toContain(teamId);
    expect(JSON.stringify(snapshot)).toContain(name);
  } finally {
    await api.closeTab(managerId).catch(() => {});
    await api.closeWorkspace(workspace.id).catch(() => {});
  }
});
