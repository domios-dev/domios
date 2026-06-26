import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, type Page } from "@playwright/test";
import type { ApiClient } from "../../helpers/api-client";
import { test } from "../../helpers/fixtures";
import {
  focusTaskById,
  isEmpty,
  openCanvas,
  pickProbeTaskId,
  setCanvasScaleOnTask,
} from "../../helpers/canvas-inspect";

type Box = { width: number; height: number };
type Sample = { requested: number; actualScale: number; outer: Box; slot: Box; crispCount: number };

interface CreatePlanResponse {
  plan_id: string;
  plan?: { sections?: { id: string }[] };
}

async function seedZoomDriftPlan(api: ApiClient, projectPath: string): Promise<void> {
  const created = await api.post<CreatePlanResponse>("/plans", {
    title: "Canvas Zoom Drift Synthetic Plan",
    project_path: projectPath,
    owner: { pane_id: "zoom-drift-orchestrator", session_id: null, title: "Zoom Drift Orchestrator" },
    kinds: "progress:#38bdf8",
  });
  const section = await api.post<{ section_id: string }>(`/plans/${created.plan_id}/sections`, {
    title: "Readable zoom drift section",
    body: "A stable section body keeps the plan node readable while the task card is measured.",
  });
  await api.post<{ task_id: string }>(`/plans/${created.plan_id}/tasks`, {
    name: "Measure stable title slot across zoom levels",
    type: "implement",
    sections: section.section_id,
    model: "codex:opus",
    why: "The drift probe needs a deterministic task card that reaches readable canvas bands.",
    body: "The title slot and outer footprint should scale linearly under one camera transform.",
    intent: "Detect per-card zoom/counter-scale geometry jumps.",
    parent: null,
  });
}

async function readbackScale(page: Page, taskId: string): Promise<number> {
  return page.evaluate((id) => {
    const registry = window.__codetermCanvasScene;
    for (const slot of registry?.list() ?? []) {
      const readback = registry?.get(slot);
      const world = readback?.nodeWorldBox(id);
      const screen = readback?.nodeScreenBox(id);
      if (world && screen && world.width > 0) return screen.width / world.width;
    }
    return null;
  }, taskId).then((scale) => {
    expect(scale, "actual camera scale from canvas scene readback").not.toBeNull();
    return scale ?? 1;
  });
}

async function sampleTask(page: Page, taskId: string, requested: number): Promise<Sample> {
  await setCanvasScaleOnTask(page, taskId, requested);
  await page.waitForTimeout(180);
  const actualScale = await readbackScale(page, taskId);
  const sample = await page.evaluate((id) => {
    const card = document.querySelector(`[data-testid="canvas-task-card"][data-task-id="${id}"]`) as HTMLElement | null;
    const slot = card?.firstElementChild as HTMLElement | null;
    if (!card || !slot) return null;
    const outerRect = card.getBoundingClientRect();
    const slotRect = slot.getBoundingClientRect();
    return {
      outer: { width: outerRect.width, height: outerRect.height },
      slot: { width: slotRect.width, height: slotRect.height },
      crispCount: document.querySelectorAll(".ct-canvas-crisp").length,
    };
  }, taskId);
  expect(sample, `task ${taskId} rendered at requested scale ${requested}`).not.toBeNull();
  return { requested, actualScale, ...sample! };
}

function normalized(sample: Sample) {
  return {
    requested: sample.requested,
    actualScale: sample.actualScale,
    outerW: sample.outer.width / sample.actualScale,
    outerH: sample.outer.height / sample.actualScale,
    slotW: sample.slot.width / sample.actualScale,
    slotH: sample.slot.height / sample.actualScale,
  };
}

function closeRatio(actual: number, expected: number, label: string): void {
  const tolerance = Math.max(1, Math.abs(expected) * 0.03);
  expect(Math.abs(actual - expected), label).toBeLessThanOrEqual(tolerance);
}

test("@golden @canvas-zoom-drift task slot rects stay linear across camera scales", async ({ api, ui }) => {
  test.setTimeout(120_000);
  const projectPath = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-canvas-zoom-drift-"));
  await api.createWorkspace("canvas zoom drift", projectPath);
  const workspace = (await api.workspaces()).find((w) => w.cwd === projectPath || w.name === "canvas zoom drift");
  if (workspace) await api.focusWorkspace(workspace.id);
  await seedZoomDriftPlan(api, projectPath);

  const { page } = ui;
  await page.reload({ waitUntil: "domcontentloaded" });

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "no active plans in the dev workspace - seed a plan to verify zoom drift");
    return;
  }

  const taskId = await pickProbeTaskId(page);
  if (!taskId) {
    test.skip(true, "plan has no tasks to probe zoom drift");
    return;
  }

  await focusTaskById(page, taskId, 1);
  const samples: Sample[] = [];
  for (const scale of [1, 1.25, 1.5, 2, 4]) {
    samples.push(await sampleTask(page, taskId, scale));
  }

  const ratios = samples.map(normalized);
  console.log(`canvas-zoom-drift ratios ${JSON.stringify(ratios)}`);
  for (const sample of samples) {
    expect(sample.crispCount, `.ct-canvas-crisp absent at requested ${sample.requested}`).toBe(0);
  }

  const base = ratios[0];
  for (const ratio of ratios.slice(1)) {
    closeRatio(ratio.outerW, base.outerW, `outer width/scale stable at ${ratio.requested}`);
    closeRatio(ratio.outerH, base.outerH, `outer height/scale stable at ${ratio.requested}`);
    closeRatio(ratio.slotW, base.slotW, `inner grid width/scale stable at ${ratio.requested}`);
    closeRatio(ratio.slotH, base.slotH, `inner grid height/scale stable at ${ratio.requested}`);
  }
});
