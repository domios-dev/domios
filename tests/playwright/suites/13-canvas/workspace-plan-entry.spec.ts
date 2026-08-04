/**
 * WHAT: Workspace plan entry opens canvas/web plan surfaces from the sidebar flow.
 * WHY: Plans must remain reachable from workspace navigation after canvas mount refactors.
 * ORACLE: Sidebar expansion, plan entry activation, and canvas web-page lifecycle expose the expected plan surface.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import {
  closeWorkspaceTabs,
  expandWorkspaceSidebar,
  openSidebarIfNeeded,
} from "../../helpers/canvas-inspect";
import { closeCanvasWebSession, openCanvasWebPage } from "../../helpers/canvas-web-page";

interface CreatePlanResponse {
  plan_id: string;
}

test("@golden @workspace-plan-entry sidebar Plan entry opens workspace canvas", async ({ api, webBrowser, tabCleanup }) => {
  test.setTimeout(120_000);
  const projectPath = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-workspace-plan-entry-")));
  const wsName = "workspace plan entry gate";
  await api.createWorkspace(wsName, projectPath);
  const workspace = (await api.workspaces()).find((w) => w.cwd === projectPath);
  if (!workspace) throw new Error("created workspace was not listed");

  const created = await api.post<CreatePlanResponse>("/plans", {
    title: "Workspace Plan Entry Synthetic Plan",
    project_path: projectPath,
    owner: { tab_id: "plan-entry-orchestrator", session_id: null, title: "Plan Entry Orchestrator" },
    kinds: "progress:#38bdf8",
  });
  const section = await api.post<{ section_id: string }>(`/plans/${created.plan_id}/sections`, {
    title: "Entry gate section",
    body: "Workspace plan index needs non-empty plan content.",
  });
  await api.post(`/plans/${created.plan_id}/mode`, { mode: "approved" });
  await api.post(`/plans/${created.plan_id}/tasks`, {
    name: "Entry gate task",
    type: "research",
    sections: section.section_id,
    body: "Task body for workspace plan index.",
    parent: null,
  });

  await api.focusWorkspace(workspace.id);
  await closeWorkspaceTabs(api, workspace.id);
  await expect
    .poll(async () => {
      const body = await api.get<{ plans: Array<{ plan_id: string; file_path?: string; task_count?: number }> }>("/plans");
      const row = body.plans.find((p) => (p.file_path ?? "").includes(projectPath));
      return row != null && (row.task_count ?? 0) > 0;
    })
    .toBe(true);

  const session = await openCanvasWebPage(webBrowser, { clearHarness: true });
  try {
    const { page } = session;
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
    await openSidebarIfNeeded(page);
    const wsItem = page.locator(`[data-testid="workspace-item"][data-workspace-id="${workspace.id}"]`);
    await expect(wsItem).toBeVisible({ timeout: 30_000 });
    await wsItem.click();
    await expandWorkspaceSidebar(page, workspace.id);

    await expect
      .poll(
        async () =>
          page.evaluate(async (cwd) => {
            const token = localStorage.getItem(`codeterm-web-token:${window.location.origin}`) || "";
            const r = await fetch("/api/plans", { headers: { Authorization: `Bearer ${token}` } });
            if (!r.ok) return false;
            const body = (await r.json()) as { plans?: Array<{ file_path?: string; task_count?: number }> };
            return (body.plans ?? []).some(
              (p) => (p.file_path ?? "").includes(cwd) && (p.task_count ?? 0) > 0,
            );
          }, projectPath),
        { timeout: 60_000 },
      )
      .toBe(true);

    const planTabId = `workspace-plan:${workspace.id}`;
    const entry = page.locator(`[data-testid="workspace-plan-entry"][data-tab-id="${planTabId}"]`);
    await expect(entry).toBeVisible({ timeout: 30_000 });
    await expect(entry).toHaveAttribute("data-workspace-action", "plan");
    await expect(entry).toHaveAttribute("data-tab-kind", "workspace-plan");
    await expect(entry).not.toHaveAttribute("role", "tab");
    await expect(entry).not.toHaveAttribute("aria-selected");
    await expect(entry).not.toHaveAttribute("aria-current");
    await entry.click();

    await expect(page.locator('[data-testid="plan-view-shell"]').first()).toBeVisible({ timeout: 30_000 });
    await expect(page.locator('[data-testid="plan-board-view"]')).toBeVisible({ timeout: 30_000 });
    await expect(entry).toHaveAttribute("data-active", "true");
    await expect(entry).toHaveAttribute("aria-current", "page");
    await expect(page.getByTitle("Close plan canvas")).toHaveCount(0);

    const tab = await tabCleanup.createTab<{ id: string; tab_id?: string }>({ workspace_id: workspace.id });
    if (tab.tab_id) {
      await page.locator(`[data-testid="tab-item"][data-tab-id="${tab.tab_id}"]`).click();
    }
    await expect(page.locator('[data-testid="plan-view-shell"]').first()).toBeHidden({ timeout: 30_000 });
    await expect(entry).toHaveAttribute("data-active", "false");
    await expect(entry).not.toHaveAttribute("aria-current");
  } finally {
    await closeCanvasWebSession(session);
  }
});
