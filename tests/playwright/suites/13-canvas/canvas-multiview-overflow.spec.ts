/**
 * 13-canvas - multi-view overflow regression across canvas mount contexts.
 *
 * Exercises two real UI surfaces where clipped canvas content has regressed:
 *   1. Inline workspace board (no active tab, mode=workspace) @ desktop viewport
 *   2. Pane task strip opens workspace canvas overlay @ narrow viewport
 *
 * Deterministic synthetic long-content plan; assertions use visible DOM geometry only.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden --grep @canvas-multiview-overflow
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { ApiClient } from "../../helpers/api-client";
import { expect, test } from "../../helpers/fixtures";
import {
  assertVisibleCanvasGeometryClean,
  canvasScale,
  closePanePlanCanvas,
  openInlineWorkspaceCanvas,
  openPanePlanCanvas,
  setCanvasScale,
} from "../../helpers/canvas-inspect";

const SCALES = [0.5, 1.0, 1.75] as const;
const WORKSPACE_CANVAS = '[data-testid="canvas-view"]';

interface MuxState {
  workspaces: Array<{ id: string; tabs: Array<{ id: string }> }>;
}

interface PlanResponse {
  primary_spec_id?: string | null;
  sections?: { id: string }[];
  tasks?: { id: string }[];
}

interface CreatePlanResponse {
  plan_id: string;
  plan?: {
    primary_spec_id?: string | null;
    sections?: { id: string }[];
  };
}

function longSectionBody(): string {
  const lines = Array.from(
    { length: 120 },
    (_, i) => `${i + 1}. Multiview overflow paragraph with enough prose to wrap inside the spec node viewport.`,
  ).join("\n");
  const code = [
    "```ts",
    ...Array.from({ length: 60 }, (_, i) => `export const multiviewLine${i + 1} = "overflow-regression-${i + 1}";`),
    "```",
  ].join("\n");
  return [`# Multiview Section`, lines, code].join("\n\n");
}

function longTaskBody(): string {
  return [
    "Task body with a long fenced block so card sizing cannot collapse to an empty shell in pane or workspace views.",
    "",
    "```rust",
    ...Array.from({ length: 70 }, (_, i) => `fn multiview_case_${i + 1}() -> &'static str { "line-${i + 1}" }`),
    "```",
  ].join("\n");
}

async function seedSyntheticPlan(api: ApiClient, projectPath: string): Promise<string> {
  const spec = await api.post<{ spec_id: string }>("/specs", {
    title: "Multiview Overflow Synthetic Spec",
    project_path: projectPath,
  });
  await api.post<{ section_id: string }>(`/specs/${spec.spec_id}/sections`, {
    title: "Long markdown and code for overflow checks",
    body: longSectionBody(),
  });
  const created = await api.post<CreatePlanResponse>("/plans", {
    title: "Multiview Overflow Synthetic Plan",
    project_path: projectPath,
    owner: { pane_id: "multiview-orchestrator", session_id: null, title: "Multiview Orchestrator" },
    from_spec: spec.spec_id,
  });
  const planId = created.plan_id;
  const planSectionId = created.plan?.sections?.[0]?.id;
  if (!planSectionId) throw new Error("from_spec did not create a mirrored plan section");
  const task = await api.post<{ task_id: string }>(`/plans/${planId}/tasks`, {
    name: "Guard multiview canvas overflow",
    type: "implement",
    sections: planSectionId,
    body: longTaskBody(),
    intent: "Rendered content must stay inside measured nodes in every canvas mount context.",
    parent: null,
  });
  await expect
    .poll(async () => {
      const plan = await api.get<PlanResponse>(`/plans/${planId}`);
      return {
        hasSpec: plan.primary_spec_id === spec.spec_id,
        hasTask: plan.tasks?.some((t) => t.id === task.task_id) ?? false,
      };
    })
    .toEqual({ hasSpec: true, hasTask: true });
  return planId;
}

async function closeAllTabs(api: ApiClient, workspaceId: string): Promise<void> {
  for (let pass = 0; pass < 8; pass++) {
    const mux = await api.get<MuxState>("/mux/state");
    const ws = mux.workspaces.find((w) => w.id === workspaceId);
    const tabs = [...(ws?.tabs ?? [])];
    if (tabs.length === 0) return;
    for (const tab of tabs) {
      await api.post(`/workspaces/${workspaceId}/tabs/${tab.id}/close`);
    }
  }
  const mux = await api.get<MuxState>("/mux/state");
  const remaining = mux.workspaces.find((w) => w.id === workspaceId)?.tabs.length ?? 0;
  expect(remaining, "workspace tabs closed for inline canvas").toBe(0);
}

async function auditAtScales(
  page: import("@playwright/test").Page,
  viewLabel: string,
  rootSelector: string | undefined,
): Promise<void> {
  await setCanvasScale(page, 1.6);
  await expect(page.locator('[data-testid="canvas-spec-section-body"]').first()).toBeVisible({ timeout: 30_000 });
  for (const scale of SCALES) {
    await setCanvasScale(page, scale);
    const actual = await canvasScale(page);
    expect(actual, `${viewLabel} scale ${scale}`).toBeGreaterThanOrEqual(scale - 0.03);
    expect(actual, `${viewLabel} scale ${scale}`).toBeLessThanOrEqual(scale + 0.03);
    await assertVisibleCanvasGeometryClean(page, `${viewLabel}@${scale}`, rootSelector);
  }
}

test("@golden @canvas-multiview-overflow synthetic plan has no visible overflow in workspace and pane plan views", async ({ api, ui, paneCleanup }) => {
  test.setTimeout(120_000);
  const { page } = ui;
  const projectPath = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-canvas-multiview-"));
  await api.createWorkspace("canvas multiview overflow", projectPath);
  const workspace = (await api.workspaces()).find((w) => w.cwd === projectPath);
  if (!workspace) throw new Error("created workspace was not listed");
  const planId = await seedSyntheticPlan(api, projectPath);

  await api.focusWorkspace(workspace.id);
  await closeAllTabs(api, workspace.id);
  await page.reload({ waitUntil: "domcontentloaded" });

  await page.setViewportSize({ width: 1280, height: 800 });
  await openInlineWorkspaceCanvas(page);
  await expect(page.locator('[data-testid="canvas-plan-node"]')).toHaveCount(1, { timeout: 30_000 });
  await expect(page.locator('[data-testid="canvas-task-card"]')).toHaveCount(1, { timeout: 30_000 });
  await auditAtScales(page, "workspace-inline", undefined);

  const pane = await paneCleanup.createPane<{ id: string; tab_id?: string }>({ workspace_id: workspace.id });
  await api.post(`/plans/${planId}/claim`, {
    owner: { pane_id: pane.id, session_id: null, title: "Multiview Pane Owner" },
  });
  if (pane.tab_id) {
    await page.locator(`[data-testid="tab-item"][data-tab-id="${pane.tab_id}"]`).click();
  }
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.setViewportSize({ width: 960, height: 640 });
  await openPanePlanCanvas(page, pane.id);
  await expect(page.locator(WORKSPACE_CANVAS).first()).toBeVisible();
  await expect(page.locator(`${WORKSPACE_CANVAS} [data-testid="canvas-plan-node"]`).first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(`${WORKSPACE_CANVAS} [data-testid="canvas-task-card"]`).first()).toBeVisible({ timeout: 30_000 });
  await auditAtScales(page, "pane-workspace-overlay", WORKSPACE_CANVAS);

  await closePanePlanCanvas(page, pane.id);
  await expect(page.locator('[data-testid="pane-plan-overlay"]')).toHaveCount(0);
});
