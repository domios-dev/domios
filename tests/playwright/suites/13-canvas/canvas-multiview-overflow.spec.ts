/**
 * 13-canvas - multi-view overflow regression across canvas mount contexts.
 *
 * Exercises two real UI surfaces where clipped canvas content has regressed:
 *   1. Inline workspace board (no active tab, mode=workspace) @ desktop viewport
 *   2. Inline workspace board @ narrow viewport (same mount, overflow regression)
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
  assertReadbackNodesOnScreen,
  canvasScale,
  closeWorkspaceTabs,
  openInlineWorkspaceCanvas,
  setCanvasScale,
  waitForReadbackCounts,
} from "../../helpers/canvas-inspect";
import { closeCanvasWebSession, openCanvasWebPage } from "../../helpers/canvas-web-page";

const SCALES = [0.5, 1.0, 1.75] as const;

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

async function seedSyntheticPlan(api: ApiClient, projectPath: string): Promise<{ planId: string; taskId: string }> {
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
  return { planId, taskId: task.task_id };
}

async function closeAllTabs(api: ApiClient, workspaceId: string): Promise<void> {
  await closeWorkspaceTabs(api, workspaceId);
}

async function auditAtScales(
  page: import("@playwright/test").Page,
  viewLabel: string,
): Promise<void> {
  await setCanvasScale(page, 1.6);
  await waitForReadbackCounts(page, 1, 1);
  for (const scale of SCALES) {
    await setCanvasScale(page, scale);
    const actual = await canvasScale(page);
    expect(actual, `${viewLabel} scale ${scale}`).toBeGreaterThanOrEqual(scale - 0.03);
    expect(actual, `${viewLabel} scale ${scale}`).toBeLessThanOrEqual(scale + 0.03);
    await assertReadbackNodesOnScreen(page);
  }
}

test("@golden @canvas-multiview-overflow synthetic plan has no visible overflow in workspace and pane plan views", async ({ api, webBrowser }) => {
  test.setTimeout(120_000);
  const projectPath = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-canvas-multiview-"));
  await api.createWorkspace("canvas multiview overflow", projectPath);
  const workspace = (await api.workspaces()).find((w) => w.cwd === projectPath);
  if (!workspace) throw new Error("created workspace was not listed");
  const { planId: _planId, taskId: _taskId } = await seedSyntheticPlan(api, projectPath);

  await api.focusWorkspace(workspace.id);
  await closeAllTabs(api, workspace.id);

  const session = await openCanvasWebPage(webBrowser, { clearHarness: true });
  try {
    const { page } = session;
    await page.setViewportSize({ width: 1280, height: 800 });
    await openInlineWorkspaceCanvas(page, workspace.id);
    await waitForReadbackCounts(page, 1, 1);
    await auditAtScales(page, "workspace-inline");

    await page.setViewportSize({ width: 960, height: 640 });
    await openInlineWorkspaceCanvas(page, workspace.id);
    await waitForReadbackCounts(page, 1, 1);
    await auditAtScales(page, "workspace-inline-narrow");

    await expect(page.locator('[data-testid="pane-plan-overlay"]')).toHaveCount(0);
  } finally {
    await closeCanvasWebSession(session);
  }
});
