/**
 * WHAT: A real Team plan renders every task-status bucket across sidebar widths.
 * WHY: Narrow Team headers must not hide task counts when the title column tightens.
 * ORACLE: Visible bucket counts, colors, and geometry match the plan fixture.
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

interface TabResponse {
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
  if (compact === true) expect(compactDisplay, "compact bucket dot is visible").not.toBe("none");
  if (compact === false) expect(compactDisplay, "full bucket dot is hidden").toBe("none");

  for (const [key, count] of Object.entries(COUNTS)) {
    const item = tally.getByTestId(`team-status-${key}`);
    await expect(item).toBeVisible();
    await expect(item).toContainText(String(count));
    await expect(item).toHaveCSS("color", COLORS[key as keyof typeof COLORS]);
    await expect(item).not.toHaveCSS("text-overflow", "ellipsis");
    await expect(item.locator(".sidebar-team-readout-compact-dot")).toHaveCSS(
      "display",
      compactDisplay,
    );
    expect(await item.locator(".sidebar-team-readout-label").textContent(), "count and label retain a text gap").toBe(` ${key}`);
    if (compact === false) {
      await expect(item.locator(".sidebar-team-readout-label")).toHaveCSS("margin-left", "3px");
    }
  }

  const geometry = await tally.evaluate((element) => {
    const tallyBox = element.getBoundingClientRect();
    const unitBox = element.closest('[data-testid="team-unit"]')?.getBoundingClientRect();
    const itemBoxes = Array.from(element.querySelectorAll('[data-testid^="team-status-"]')).map((item) => item.getBoundingClientRect());
    return {
      clientWidth: element.clientWidth,
      scrollWidth: element.scrollWidth,
      centered: unitBox ? Math.abs((tallyBox.left + tallyBox.right) - (unitBox.left + unitBox.right)) <= 4 : false,
      visible: itemBoxes.every((box) => box.left >= tallyBox.left && box.right <= tallyBox.right),
    };
  });
  expect(geometry.scrollWidth, "tally has no horizontal overflow").toBeLessThanOrEqual(geometry.clientWidth);
  expect(geometry.visible, "every bucket stays inside the tally viewport").toBe(true);
  expect(geometry.centered, "tally is centered in the Team unit").toBe(true);
}

async function assertTeamHeaderGeometry(teamUnit: Locator): Promise<void> {
  const geometry = await teamUnit.evaluate((unit) => {
    const header = unit.querySelector(".sidebar-team-header")?.getBoundingClientRect();
    const donut = unit.querySelector('[data-testid="team-donut"]')?.getBoundingClientRect();
    const title = unit.querySelector(".sidebar-team-title-block")?.getBoundingClientRect();
    const chevron = unit.querySelector('[data-testid="team-collapse-toggle"]')?.getBoundingClientRect();
    const kebab = unit.querySelector('[data-testid="team-menu-trigger"]')?.getBoundingClientRect();
    const slot = unit.querySelector('[data-testid="team-progress-slot"]')?.getBoundingClientRect();
    const hole = unit.querySelector('[data-testid="team-donut-hole"]')?.getBoundingClientRect();
    const donutCenter = donut && hole ? document.elementFromPoint(donut.left + donut.width / 2, donut.top + donut.height / 2) : null;
    return { header, donut, title, chevron, kebab, slot, hole, centerIsHole: donutCenter?.getAttribute("data-testid") === "team-donut-hole", hasInnerIcon: Boolean(unit.querySelector('[data-testid="team-donut"] svg')) };
  });
  expect(geometry.donut?.width, "donut is 22px").toBe(22);
  expect(geometry.donut?.height, "donut is 22px").toBe(22);
  expect(geometry.hasInnerIcon, "donut remains hollow without an icon").toBe(false);
  expect(geometry.hole?.width, "donut center is visibly punctured").toBe(12);
  expect(geometry.centerIsHole, "center pixel belongs to the empty hole, not a color segment").toBe(true);
  expect(geometry.title && geometry.donut && Math.abs(geometry.title.y - geometry.donut.y) < geometry.donut.height, "title shares header row with identity").toBe(true);
  expect(geometry.chevron && geometry.donut && geometry.donut.x - geometry.chevron.right, "donut keeps its deliberate inset after the restored chevron").toBe(4);
  expect(geometry.chevron?.width, "restored chevron glyph control is 20px wide").toBe(20);
  expect(geometry.kebab && geometry.slot && geometry.kebab.x === geometry.slot.x && geometry.kebab.width === geometry.slot.width, "kebab occupies the fixed progress slot").toBe(true);
}

async function assertTeamTitleFits(teamUnit: Locator): Promise<void> {
  const title = teamUnit.getByTestId("team-name");
  await expect(title).toHaveText("Team UI QA");
  const geometry = await title.evaluate((element) => {
    const unit = element.closest('[data-testid="team-unit"]')?.getBoundingClientRect();
    const header = element.closest(".sidebar-team-header")?.getBoundingClientRect();
    const sidebar = element.closest('[data-testid="sidebar"]');
    return {
      clientWidth: element.clientWidth,
      scrollWidth: element.scrollWidth,
      text: element.textContent,
      unitWidth: unit?.width,
      sidebarWidth: sidebar?.clientWidth,
      headerInset: unit && header ? unit.width - header.width : undefined,
    };
  });
  expect(geometry.text, "Team fixture title remains intact").toBe("Team UI QA");
  expect(geometry.scrollWidth, "Team fixture title has no ellipsis or clipping").toBeLessThanOrEqual(geometry.clientWidth);
  expect(geometry.unitWidth, "Team unit consumes the sidebar's full available width").toBe(geometry.sidebarWidth);
  expect(geometry.headerInset, "header no longer adds horizontal inset inside the tint block").toBe(0);
}

async function assertOverlayControlsDoNotShiftPlan(teamUnit: Locator): Promise<void> {
  const boxes = async () => teamUnit.evaluate((unit) => {
    const box = (selector: string) => {
      const rect = unit.querySelector(selector)?.getBoundingClientRect();
      return rect && { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    };
    return {
      title: box('.sidebar-team-title-block'),
      subtitle: box('[data-testid="team-effort"]'),
      donut: box('[data-testid="team-donut"]'),
      tally: box('[data-testid="team-status-tally"]'),
    };
  });
  const restSlots = await teamUnit.evaluate((unit) => {
    const header = unit.querySelector('.sidebar-team-header') as HTMLElement;
    const kebab = unit.querySelector('[data-testid="team-menu-trigger"]') as HTMLElement;
    const count = unit.querySelector('[data-testid="team-done-count"]') as HTMLElement;
    const slot = unit.querySelector('[data-testid="team-progress-slot"]') as HTMLElement;
    const title = unit.querySelector('.sidebar-team-title-block') as HTMLElement;
    return {
      columns: getComputedStyle(header).gridTemplateColumns,
      hasChevron: Boolean(unit.querySelector('[data-testid="team-collapse-toggle"], .sidebar-team-chevron')),
      kebabBox: kebab.getBoundingClientRect().toJSON(),
      countBox: count.getBoundingClientRect().toJSON(),
      slotBox: slot.getBoundingClientRect().toJSON(),
      countOpacity: getComputedStyle(count).opacity,
      titleWidth: title.getBoundingClientRect().width,
    };
  });
  const before = await boxes();
  await teamUnit.locator(".sidebar-team-header").hover();
  const onHover = await boxes();
  await teamUnit.getByTestId("team-menu-trigger").focus();
  const onFocus = await boxes();
  expect(onHover, "hover reveal preserves every Team geometry box").toEqual(before);
  expect(onFocus, "focus reveal preserves every Team geometry box").toEqual(before);

  expect(restSlots.columns.split(" "), "rest grid reserves chevron, card activation, and same-slot progress").toHaveLength(3);
  expect(restSlots.hasChevron, "Team header restores the collapse chevron").toBe(true);
  expect(restSlots.kebabBox, "kebab and done/total have identical fixed-slot bounds").toEqual(restSlots.slotBox);
  expect(restSlots.countBox, "done/total and kebab have identical fixed-slot bounds").toEqual(restSlots.slotBox);
  expect(restSlots.countOpacity, "done/total is visible at rest").toBe("1");
  expect(restSlots.titleWidth, "220px title receives reclaimed control width").toBeGreaterThan(130);
}

test("@golden @team-status-tally Team status tally keeps every bucket visible across sidebar widths", async ({ api, webBrowser, tabCleanup }, testInfo) => {
  const projectPath = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-team-status-tally-")));
  const workspaceName = "team status tally";
  await api.createWorkspace(workspaceName, projectPath);
  const workspace = (await api.workspaces()).find((entry) => entry.cwd === projectPath);
  if (!workspace) throw new Error("fixture workspace was not created");

  const tab = await tabCleanup.createTab<TabResponse>({ workspace_id: workspace.id, cwd: projectPath, title: "Team tally owner" });
  await api.post(`/tabs/${tab.id}/role`, { role: "Orchestrator" });
  const created = await api.post<PlanResponse>("/plans", {
    title: "Team tally fixture",
    project_path: projectPath,
    owner: { tab_id: tab.id, session_id: null, title: "Team tally owner" },
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
      ...(status === "in_progress" ? { lane: { tab_id: tab.id } } : {}),
      parent: null,
    });
    if (status) {
      await api.request("PATCH", `/plans/${created.plan_id}/tasks/${task.task_id}`, {
        status,
        ...(status === "blocked" ? { status_label: "fixture" } : {}),
      });
    }
  };
  for (const [name, status] of [
    ["todo pending"],
    ["todo active", "in_progress"],
    ["todo queued"],
    ["review", "in_review"],
    ["blocked", "blocked"],
    ["done one", "done"],
    ["done two", "done"],
    ["done three", "done"],
  ]) {
    await addTask(name, status);
  }

  await api.focusWorkspace(workspace.id);
  const session = await openTallyPage(webBrowser);
  try {
    const { page } = session;
    await expect(page.getByTestId("general-section")).toBeVisible();
    await expect(page.getByTestId("general-section-toggle")).toHaveAttribute("aria-expanded", "true");
    await expect(page.getByTestId("general-section").locator('[data-testid="workspace-group"]')).toHaveCount(0);
    await expect(page.locator('[data-testid="nav-extensions"]')).toBeVisible();
    await expect(page.locator('[data-testid="nav-memory"]')).toBeVisible();
    await expect(page.locator('[data-testid="nav-mesh"]')).toBeVisible();
    const sidebarWidths = await page.getByTestId("sidebar").evaluate((sidebar) => ({ clientWidth: sidebar.clientWidth, scrollWidth: sidebar.scrollWidth }));
    expect(sidebarWidths.scrollWidth, "sidebar has no horizontal scrollbar").toBeLessThanOrEqual(sidebarWidths.clientWidth);
    await page.locator('[data-testid="workspace-item"]').filter({ hasText: workspaceName }).click();
    await expandWorkspaceSidebar(page, workspace.id);
    await expect(page.locator('[data-testid="team-unit"]')).toBeVisible({ timeout: 30_000 });

    const teamUnit = page.locator(`[data-testid="team-unit"][data-team-id="${created.team_id}"]`);
    const header = teamUnit.locator(".sidebar-team-header");
    const menuTrigger = teamUnit.getByTestId("team-menu-trigger");
    await page.getByTestId("nav-extensions").focus();
    await header.hover();
    await expect(menuTrigger).toHaveCSS("opacity", "1");
    await expect(menuTrigger).toHaveCSS("pointer-events", "auto");
    await expect(teamUnit.getByTestId("team-done-count")).toHaveCSS("opacity", "0");
    await page.mouse.move(500, 500);
    await expect(menuTrigger).toHaveCSS("opacity", "0");
    await expect(menuTrigger).toHaveCSS("pointer-events", "none");
    await expect(teamUnit.getByTestId("team-done-count")).toHaveCSS("opacity", "1");

    await menuTrigger.focus();
    await expect(menuTrigger).toHaveCSS("opacity", "1");
    await expect(menuTrigger).toHaveCSS("pointer-events", "auto");
    await page.mouse.move(500, 500);
    await expect(menuTrigger).toHaveCSS("opacity", "1");
    await expect(menuTrigger).toHaveCSS("pointer-events", "auto");
    await page.locator(`[data-testid="terminal-tab"][data-tab-id="${tab.id}"]:visible`).click({ position: { x: 10, y: 10 } });
    await expect(header).toHaveAttribute("data-menu-dismissed", "true");
    await expect(menuTrigger).toHaveCSS("opacity", "0");
    await expect(teamUnit.getByTestId("team-done-count")).toHaveCSS("opacity", "1");
    await menuTrigger.focus();
    await expect(header).not.toHaveAttribute("data-menu-dismissed");
    await expect(menuTrigger).toHaveCSS("opacity", "1");
    await expect(menuTrigger).toHaveCSS("pointer-events", "auto");
    await menuTrigger.click();
    await page.getByTestId("team-menu").getByRole("button", { name: "Rename", exact: true }).click();
    await teamUnit.getByTestId("team-name-input").fill("Team UI QA");
    await teamUnit.getByTestId("team-name-input").press("Enter");
    await expect(teamUnit.getByTestId("team-name")).toHaveText("Team UI QA");
    const neutralNav = page.getByTestId("nav-extensions");
    await expect(teamUnit.getByTestId("team-collapse-toggle")).toHaveAttribute("aria-expanded", "true");
    await setSidebarWidth(session, 220);
    const neutralNavBox = await neutralNav.boundingBox();
    expect(neutralNavBox, "neutral navigation surface is measurable").not.toBeNull();
    if (!neutralNavBox) throw new Error("Neutral navigation surface is unavailable");
    const neutralPoint = { x: neutralNavBox.x + neutralNavBox.width / 2, y: neutralNavBox.y + neutralNavBox.height / 2 };
    await page.mouse.move(neutralPoint.x, neutralPoint.y);
    expect(await teamUnit.evaluate((unit, point) => !unit.contains(document.elementFromPoint(point.x, point.y)), neutralPoint), "rest geometry starts with the pointer outside the Team unit").toBe(true);
    await neutralNav.focus();
    await expect(neutralNav).toBeFocused();
    expect(await teamUnit.evaluate((unit) => !unit.contains(document.activeElement)), "rest geometry starts with focus outside the Team unit").toBe(true);
    await expect(teamUnit.getByTestId("team-done-count")).toHaveCSS("opacity", "1");
    await assertTeamTitleFits(teamUnit);
    await assertTally(teamUnit, false);
    await assertTeamHeaderGeometry(teamUnit);
    await assertOverlayControlsDoNotShiftPlan(teamUnit);
    await page.screenshot({ path: testInfo.outputPath("wave2d-team-220.png"), fullPage: false });
    await setSidebarWidth(session, 272);
    await assertTally(teamUnit, false);
    await page.screenshot({ path: testInfo.outputPath("wave2d-team-272.png"), fullPage: false });
    await setSidebarWidth(session, 160);
    await assertTally(teamUnit, true);
    await page.screenshot({ path: testInfo.outputPath("wave2d-team-160.png"), fullPage: false });
    const chevron = teamUnit.getByTestId("team-collapse-toggle");
    await chevron.press("Enter");
    await expect(chevron).toHaveAttribute("aria-expanded", "false");
    await chevron.press("Enter");
    await expect(chevron).toHaveAttribute("aria-expanded", "true");
    await teamUnit.getByTestId("team-card-activation").press("Enter");
    await expect(page.getByTestId("plan-view-shell")).toBeVisible();
  } finally {
    await closeTallyPage(session);
  }

  const touchSession = await openTallyPage(webBrowser, true);
  try {
    const { page } = touchSession;
    await page.locator('[data-testid="workspace-item"]').filter({ hasText: workspaceName }).click();
    await expandWorkspaceSidebar(page, workspace.id);
    await expect(page.locator('[data-testid="sidebar"]')).toHaveAttribute("data-touch-mode", "true");
    const teamUnit = page.locator(`[data-testid="team-unit"][data-team-id="${created.team_id}"]`);
    await assertTally(teamUnit, "content-fit");
    await expect(teamUnit.getByTestId("team-menu-trigger")).toHaveCSS("opacity", "1");
    await expect(teamUnit.getByTestId("team-done-count")).toHaveCSS("opacity", "0");
    await page.screenshot({ path: testInfo.outputPath("wave2d-team-touch.png"), fullPage: false });
  } finally {
    await closeTallyPage(touchSession);
    await api.deletePlan(created.plan_id).catch(() => {});
    await api.closeWorkspace(workspace.id).catch(() => {});
  }
});
