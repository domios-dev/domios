/**
 * Real Team + plan fixture verifies every status bucket's geometry, colors, and counts.
 * Narrow Team headers must never hide a bucket when the title column tightens.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { Browser, Locator } from "@playwright/test";
import { expect, test } from "../../helpers/fixtures";
import { readDevToken } from "../../helpers/api-client";
import { expandWorkspaceSidebar } from "../../helpers/canvas-inspect";
import { API_BASE, isHealthy } from "../../helpers/dev-console";
import type { CanvasWebSession } from "../../helpers/canvas-web-page";

const COUNTS = { todo: 3, review: 1, blocked: 1, done: 3 };
const COLORS = {
  todo: "rgba(129, 140, 248, 0.92)",
  review: "rgba(245, 158, 11, 0.92)",
  blocked: "rgba(239, 130, 130, 0.92)",
  done: "rgba(82, 180, 145, 0.9)",
};

interface PaneResponse {
  id: string;
}

interface PlanResponse {
  plan_id: string;
  team_id: string;
}

interface TaskResponse {
  task_id: string;
}

async function openTallyPage(browser: Browser, touch = false): Promise<CanvasWebSession> {
  if (!(await isHealthy())) throw new Error("dev console health check failed before Team tally test");
  const context = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    hasTouch: touch,
    isMobile: touch,
    serviceWorkers: "block",
  });
  const page = await context.newPage();
  const token = readDevToken();
  await page.addInitScript((devToken) => {
    localStorage.setItem(`codeterm-web-token:${window.location.origin}`, devToken);
  }, token);
  await page.goto(`${API_BASE}/#token=${encodeURIComponent(token)}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  return { page, context };
}

async function closeTallyPage(session: CanvasWebSession): Promise<void> {
  await session.context.close();
}

async function setSidebarWidth(session: CanvasWebSession, width: number): Promise<void> {
  await session.page.locator('[data-testid="sidebar"]').evaluate((sidebar, targetWidth) => {
    const handle = document.querySelector('button[title="Resize sidebar; click to collapse"]') as HTMLButtonElement | null;
    if (!handle) throw new Error("sidebar resize handle is missing");
    const box = sidebar.getBoundingClientRect();
    const handleBox = handle.getBoundingClientRect();
    const startX = handleBox.left + handleBox.width / 2;
    const startY = handleBox.top + handleBox.height / 2;
    handle.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, clientX: startX, clientY: startY }));
    document.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, clientX: startX + targetWidth - box.width, clientY: startY }));
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, clientX: startX + targetWidth - box.width, clientY: startY }));
  }, width);
  await expect(session.page.locator('[data-testid="sidebar"]')).toHaveCSS("width", `${width}px`);
}

async function assertTally(teamUnit: Locator, compact: boolean | "content-fit"): Promise<void> {
  const tally = teamUnit.getByTestId("team-status-tally");
  await expect(tally).toBeVisible();
  await expect(tally.locator('[data-testid^="team-status-"]')).toHaveCount(4);
  const compactDisplay = await tally.locator(".sidebar-team-readout-compact-dot").first().evaluate((dot) => getComputedStyle(dot).display);
  if (compact === "content-fit") {
    expect(["inline-block", "none"], "content-fit selects a supported bucket form").toContain(compactDisplay);
  }

  for (const [key, count] of Object.entries(COUNTS)) {
    const item = tally.getByTestId(`team-status-${key}`);
    await expect(item).toBeVisible();
    await expect(item).toContainText(String(count));
    await expect(item).toHaveCSS("color", COLORS[key as keyof typeof COLORS]);
    await expect(item).not.toHaveCSS("text-overflow", "ellipsis");
    await expect(item.locator(".sidebar-team-readout-compact-dot")).toHaveCSS(
      "display",
      compact === "content-fit" ? compactDisplay : compact ? "inline-block" : "none",
    );
  }

  const geometry = await tally.evaluate((element) => {
    const tallyBox = element.getBoundingClientRect();
    const titleBox = element.closest(".sidebar-team-title-block")?.getBoundingClientRect();
    const itemBoxes = Array.from(element.querySelectorAll('[data-testid^="team-status-"]')).map((item) => item.getBoundingClientRect());
    return {
      clientWidth: element.clientWidth,
      scrollWidth: element.scrollWidth,
      centered: titleBox ? Math.abs((tallyBox.left + tallyBox.right) - (titleBox.left + titleBox.right)) <= 2 : false,
      visible: itemBoxes.every((box) => box.left >= tallyBox.left && box.right <= tallyBox.right),
    };
  });
  expect(geometry.scrollWidth, "tally has no horizontal overflow").toBeLessThanOrEqual(geometry.clientWidth);
  expect(geometry.visible, "every bucket stays inside the tally viewport").toBe(true);
  expect(geometry.centered, "tally is centered in the Team header title column").toBe(true);
}

test("@golden @team-status-tally Team status tally keeps every bucket visible across sidebar widths", async ({ api, webBrowser, paneCleanup }) => {
  const projectPath = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-team-status-tally-")));
  const workspaceName = "team status tally";
  await api.createWorkspace(workspaceName, projectPath);
  const workspace = (await api.workspaces()).find((entry) => entry.cwd === projectPath);
  if (!workspace) throw new Error("fixture workspace was not created");

  const pane = await paneCleanup.createPane<PaneResponse>({ workspace_id: workspace.id, cwd: projectPath, title: "Team tally owner" });
  await api.post(`/panes/${pane.id}/role`, { role: "Orchestrator" });
  const created = await api.post<PlanResponse>("/plans", {
    title: "Team tally fixture",
    project_path: projectPath,
    owner: { pane_id: pane.id, session_id: null, title: "Team tally owner" },
    kinds: "implement:#38bdf8",
  });
  const section = await api.post<{ section_id: string }>(`/plans/${created.plan_id}/sections`, { title: "Tally" });
  await api.post(`/plans/${created.plan_id}/mode`, { mode: "approved" });

  const addTask = async (name: string, status?: string) => {
    const task = await api.post<TaskResponse>(`/plans/${created.plan_id}/tasks`, {
      name,
      type: "implement",
      sections: section.section_id,
      body: "fixture",
      acceptance_criteria: ["Status tally fixture is observable"],
      test_plan: "npx tsx src/components/Sidebar/SidebarWorkspaceTabs.test.tsx",
      ...(status === "in_progress" ? { lane: { pane_id: pane.id } } : {}),
      parent: null,
    });
    if (status) {
      await api.request("PATCH", `/plans/${created.plan_id}/tasks/${task.task_id}`, {
        status,
        ...(status === "blocked" ? { status_label: "fixture" } : {}),
      });
    }
  };
  await Promise.all([
    addTask("todo pending"),
    addTask("todo active", "in_progress"),
    addTask("todo queued"),
    addTask("review", "in_review"),
    addTask("blocked", "blocked"),
    addTask("done one", "done"),
    addTask("done two", "done"),
    addTask("done three", "done"),
  ]);

  await api.focusWorkspace(workspace.id);
  const session = await openTallyPage(webBrowser);
  try {
    const { page } = session;
    await page.locator('[data-testid="workspace-item"]').filter({ hasText: workspaceName }).click();
    await expandWorkspaceSidebar(page, workspace.id);
    await expect(page.locator('[data-testid="team-unit"]')).toBeVisible({ timeout: 30_000 });

    await setSidebarWidth(session, 220);
    const teamUnit = page.locator(`[data-testid="team-unit"][data-team-id="${created.team_id}"]`);
    await assertTally(teamUnit, false);
    await setSidebarWidth(session, 272);
    await assertTally(teamUnit, false);
    await setSidebarWidth(session, 160);
    await assertTally(teamUnit, true);
  } finally {
    await closeTallyPage(session);
  }

  const touchSession = await openTallyPage(webBrowser, true);
  try {
    const { page } = touchSession;
    await page.locator('[data-testid="workspace-item"]').filter({ hasText: workspaceName }).click();
    await expandWorkspaceSidebar(page, workspace.id);
    await expect(page.locator('[data-testid="sidebar"]')).toHaveAttribute("data-touch-mode", "true");
    await assertTally(page.locator(`[data-testid="team-unit"][data-team-id="${created.team_id}"]`), "content-fit");
  } finally {
    await closeTallyPage(touchSession);
    await api.deletePlan(created.plan_id).catch(() => {});
    await api.closeWorkspace(workspace.id).catch(() => {});
  }
});
