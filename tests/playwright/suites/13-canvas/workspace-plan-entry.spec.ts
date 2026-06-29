import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import {
  CANVAS_BOARD_SEL,
  clearCanvasHarnessFlags,
  closeWorkspaceTabs,
  expandWorkspaceSidebar,
  openSidebarIfNeeded,
} from "../../helpers/canvas-inspect";

interface CreatePlanResponse {
  plan_id: string;
}

test("@golden @workspace-plan-entry sidebar Plan entry opens workspace canvas", async ({ api, ui, paneCleanup }) => {
  test.setTimeout(120_000);
  const { page } = ui;
  const projectPath = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-workspace-plan-entry-"));
  const wsName = "workspace plan entry gate";
  await api.createWorkspace(wsName, projectPath);
  const workspace = (await api.workspaces()).find((w) => w.cwd === projectPath);
  if (!workspace) throw new Error("created workspace was not listed");

  await api.post<CreatePlanResponse>("/plans", {
    title: "Workspace Plan Entry Synthetic Plan",
    project_path: projectPath,
    owner: { pane_id: "plan-entry-orchestrator", session_id: null, title: "Plan Entry Orchestrator" },
  });

  await api.focusWorkspace(workspace.id);
  await closeWorkspaceTabs(api, workspace.id);
  await clearCanvasHarnessFlags(page);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

  await openSidebarIfNeeded(page);
  const wsItem = page.locator(`[data-testid="workspace-item"][data-workspace-id="${workspace.id}"]`);
  await expect(wsItem).toBeVisible({ timeout: 30_000 });
  await expandWorkspaceSidebar(page, workspace.id);

  const entry = wsItem.locator('[data-testid="workspace-plan-entry"]');
  await expect(entry).toBeVisible({ timeout: 30_000 });
  await expect(entry).toHaveAttribute("data-workspace-action", "plan");
  await expect(entry).toHaveAttribute("data-pinned", "true");
  await expect(entry).toHaveText(/Plan/);
  await entry.click();

  await expect(page.locator('[data-testid="canvas-view"]').first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(CANVAS_BOARD_SEL).first()).toBeVisible({ timeout: 30_000 });
  await expect(entry).toHaveAttribute("data-active", "true");
  await expect(page.getByTitle("Close plan canvas")).toHaveCount(0);

  const pane = await paneCleanup.createPane<{ id: string; tab_id?: string }>({ workspace_id: workspace.id });
  if (pane.tab_id) {
    await page.locator(`[data-testid="tab-item"][data-tab-id="${pane.tab_id}"]`).click({ force: true });
  }
  await expect(page.locator('[data-testid="canvas-view"]').first()).toBeHidden({ timeout: 30_000 });
  await expect(entry).toHaveAttribute("data-active", "false");
});
