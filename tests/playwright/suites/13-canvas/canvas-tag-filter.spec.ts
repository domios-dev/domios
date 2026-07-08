/**
 * WHAT: canvas tag filter covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { ApiClient } from "../../helpers/api-client";
import { expect, test } from "../../helpers/fixtures";
import { isEmpty, openCanvas } from "../../helpers/canvas-inspect";

interface CreatePlanResponse {
  plan_id: string;
}

async function seedTagFilterPlan(api: ApiClient, projectPath: string): Promise<void> {
  const created = await api.post<CreatePlanResponse>("/plans", {
    title: "Canvas Tag Filter Synthetic Plan",
    project_path: projectPath,
    owner: { pane_id: "tag-filter-orchestrator", session_id: null, title: "Tag Filter Orchestrator" },
  });
  await api.post(`/plans/${created.plan_id}/tag-facets`, {
    facets: [
      {
        key: "area",
        label: "Area",
        color_lane: true,
        values: [
          { v: "api", label: "API", color: "#38bdf8" },
          { v: "ui", label: "UI", color: "#34d399" },
        ],
      },
    ],
  });
  await api.post(`/plans/${created.plan_id}/tasks`, {
    name: "Implement API facet lane",
    type: "implement",
    body: "Synthetic task that should highlight when the API facet is selected.",
    tags: ["area:api"],
  });
  await api.post(`/plans/${created.plan_id}/tasks`, {
    name: "Implement UI facet lane",
    type: "implement",
    body: "Synthetic task that should dim when the API facet is selected.",
    tags: ["area:ui"],
  });
}

test("@golden @canvas-tag-filter legend click highlights matching tasks and dims others", async ({ api, ui }) => {
  const { page } = ui;

  const projectPath = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-canvas-tags-"));
  await api.createWorkspace("canvas tag filter", projectPath);
  const workspace = (await api.workspaces()).find((w) => w.cwd === projectPath);
  if (!workspace) throw new Error("created workspace was not listed");
  await api.focusWorkspace(workspace.id);
  await seedTagFilterPlan(api, projectPath);
  await page.reload({ waitUntil: "domcontentloaded" });

  await openCanvas(page);
  if (await isEmpty(page)) {
    throw new Error("synthetic tag-filter plan did not render on the canvas");
  }

  const legend = page.locator('[data-testid="canvas-tag-legend"]');
  if ((await legend.count()) === 0) {
    throw new Error("synthetic color-lane facet did not render a tag legend");
  }

  const value = legend.locator('[data-testid="canvas-tag-legend-value"]').first();
  await expect(value).toBeVisible();
  const tag = await value.getAttribute("data-tag");
  if (!tag) {
    throw new Error("legend value missing data-tag");
  }

  const cards = page.locator('[data-testid="canvas-task-card"]');
  const cardCount = await cards.count();
  if (cardCount === 0) {
    throw new Error("synthetic tag-filter tasks did not render on the board");
  }

  await value.click();
  await expect(value).toHaveAttribute("data-selected", "true");

  const highlighted = page.locator('[data-testid="canvas-task-card"][data-tag-filter="highlight"]');
  const dimmed = page.locator('[data-testid="canvas-task-card"][data-tag-filter="dimmed"]');
  await expect(highlighted.first()).toBeVisible({ timeout: 5000 });

  const highlightCount = await highlighted.count();
  const dimCount = await dimmed.count();
  expect(highlightCount + dimCount, "filtered tasks partition the board").toBe(cardCount);
  expect(highlightCount, "at least one task matches the facet filter").toBeGreaterThan(0);
  expect(dimCount, "at least one task does not match the facet filter").toBeGreaterThan(0);

  const tileTags = page.locator('[data-testid="canvas-task-tile-tags"]');
  if ((await tileTags.count()) > 0) {
    await expect(tileTags.first()).toBeVisible();
  }

  await value.click();
  await expect(value).toHaveAttribute("data-selected", "false");
  await expect(page.locator('[data-testid="canvas-task-card"][data-tag-filter="dimmed"]')).toHaveCount(0);
});
