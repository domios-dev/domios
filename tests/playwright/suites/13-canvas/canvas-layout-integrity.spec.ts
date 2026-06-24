/**
 * 13-canvas — deterministic layout integrity fixture for Canvas v2.
 *
 * Seeds one synthetic plan in a temporary workspace, then checks rendered DOM
 * geometry at multiple canvas zoom scales. This catches empty node shells,
 * clipped long markdown/code content, node overlap, and detached edge anchors.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { ApiClient } from "../../helpers/api-client";
import { expect, test } from "../../helpers/fixtures";
import {
  assertClean,
  auditCanvas,
  canvasScale,
  openCanvas,
  setCanvasScale,
} from "../../helpers/canvas-inspect";

const SCALES = [0.25, 0.5, 1.0, 1.75] as const;

interface PlanResponse {
  tasks?: { id: string }[];
}

function longSectionBody(): string {
  const lines = Array.from(
    { length: 200 },
    (_, i) => `${i + 1}. Layout integrity paragraph with enough prose to wrap inside the spec node viewport.`,
  ).join("\n");
  const mermaid = [
    "```mermaid",
    "flowchart LR",
    "  plan[Plan node] --> row[Section row]",
    "  row --> task[Task card]",
    "  task --> milestone((Milestone))",
    "```",
  ].join("\n");
  const code = [
    "```ts",
    ...Array.from({ length: 80 }, (_, i) => `export const generatedLine${i + 1} = "pagination-regression-${i + 1}";`),
    "```",
  ].join("\n");
  return [`# Huge Section`, lines, mermaid, code, "| Column | Value |", "| --- | --- |", "| pagination | stable |"].join("\n\n");
}

function longTaskBody(): string {
  return [
    "The task body intentionally contains a long fenced block so task sizing cannot collapse to an empty shell.",
    "",
    "```rust",
    ...Array.from({ length: 90 }, (_, i) => `fn regression_case_${i + 1}() -> &'static str { "line-${i + 1}" }`),
    "```",
  ].join("\n");
}

async function seedSyntheticPlan(api: ApiClient, projectPath: string): Promise<void> {
  const created = await api.post<{ plan_id: string }>("/plans", {
    title: "Canvas Layout Integrity Synthetic Plan",
    project_path: projectPath,
    owner: { pane_id: "layout-orchestrator", session_id: null, title: "Layout Orchestrator" },
    kinds: "progress:#38bdf8,review:#34d399",
  });
  const planId = created.plan_id;
  const section = await api.post<{ section_id: string }>(`/plans/${planId}/sections`, {
    title: "Huge markdown, mermaid, code, and table section",
    body: longSectionBody(),
  });
  const task = await api.post<{ task_id: string }>(`/plans/${planId}/tasks`, {
    name: "Implement pagination regression guard",
    type: "implement",
    sections: section.section_id,
    model: "codex:opus",
    why: "Large markdown and code content needs a model that can reason about layout failures.",
    body: longTaskBody(),
    intent: "Ensure rendered content fills its measured node at every zoom scale.",
    parent: null,
  });
  await expect
    .poll(async () => {
      const plan = await api.get<PlanResponse>(`/plans/${planId}`);
      return plan.tasks?.some((t) => t.id === task.task_id) ?? false;
    })
    .toBe(true);
  await api.post(`/plans/${planId}/tasks/${task.task_id}/milestones`, {
    kind: "progress",
    add_kind: true,
    text: "Synthetic milestone ensures task-cluster edge anchors are rendered.",
    from: { pane_id: "layout-orchestrator", session_id: null, title: "Layout Orchestrator" },
  });
}

test("@golden @canvas-layout-integrity synthetic long-content plan has stable geometry at multi-zoom scales", async ({ api, ui }) => {
  test.setTimeout(120_000);
  const projectPath = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-canvas-layout-"));
  await api.createWorkspace("canvas layout integrity", projectPath);
  const workspaces = await api.workspaces();
  const workspace = workspaces.find((w) => w.cwd === projectPath || w.name === "canvas layout integrity");
  if (workspace) await api.focusWorkspace(workspace.id);
  await seedSyntheticPlan(api, projectPath);

  const { page } = ui;
  await page.reload({ waitUntil: "domcontentloaded" });
  await openCanvas(page);
  await expect(page.locator('[data-testid="canvas-plan-node"]')).toHaveCount(1, { timeout: 30_000 });
  await expect(page.locator('[data-testid="canvas-spec-node"]')).toHaveCount(1, { timeout: 30_000 });
  await expect(page.locator('[data-testid="canvas-task-card"]')).toHaveCount(1, { timeout: 30_000 });
  // §24.1 content-LOD: zoom IN reveals the spec section bodies inside the fixed
  // spec node (no manual mode button — camera only).
  await setCanvasScale(page, 1.6);
  await expect(page.locator('[data-testid="canvas-spec-section-body"]').first()).toBeVisible({ timeout: 30_000 });

  for (const scale of SCALES) {
    await setCanvasScale(page, scale);
    const actual = await canvasScale(page);
    expect(actual, `canvas scale ${scale}`).toBeGreaterThanOrEqual(scale - 0.03);
    expect(actual, `canvas scale ${scale}`).toBeLessThanOrEqual(scale + 0.03);
    const result = await auditCanvas(page, `layout-integrity-${scale}`);
    assertClean(result);
  }
});
