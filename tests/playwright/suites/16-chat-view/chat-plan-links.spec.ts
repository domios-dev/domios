/**
 * WHAT: Structured chat plan/task refs resolve through the global plan index and deep-link across workspaces.
 * WHY: A valid pl_/task_ mention must never navigate to the wrong project, show a false deleted state, or lose its raw id affordances.
 * ORACLE: Chat links render human titles, preserve raw ids in tooltip/copy, switch to the owning workspace, open plan-view-shell, and keep missing refs inert.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test, type PaneCleanup, type Page } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";

interface CreatePlanResponse {
  plan_id: string;
}

interface CreateSectionResponse {
  section_id: string;
}

interface CreateTaskResponse {
  task_id: string;
}

declare global {
  interface Window {
    __codetermTestChat?: {
      pushMessages: (paneId: string, messages: Array<Record<string, unknown>>) => void;
    };
    __ctCopiedPlanRef?: string;
  }
}

async function openMockChatPane(
  api: ApiClient,
  paneCleanup: PaneCleanup,
  page: Page,
  workspaceId: string,
): Promise<string> {
  const pane = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
    workspace_id: workspaceId,
    title: `pw-plan-links-${Date.now()}`,
  });
  await api.post(`/panes/${pane.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/panes/${pane.id}/chat-view`, { enabled: true });
  await api.focusPane(pane.id);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  if (pane.tab_id) {
    const tab = page.locator(`[data-testid="tab-item"][data-tab-id="${pane.tab_id}"]`).first();
    if (await tab.isVisible({ timeout: 5_000 })) {
      await tab.click();
    }
  }
  const chat = page.locator(`[data-testid="pane-session-chat"][data-pane-id="${pane.id}"]`);
  await expect(chat).toBeVisible({ timeout: 30_000 });
  return pane.id;
}

async function seedCrossWorkspacePlan(api: ApiClient) {
  const projectA = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-chat-plan-a-")));
  const projectB = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-chat-plan-b-")));
  await api.createWorkspace("chat plan links A", projectA);
  await api.createWorkspace("chat plan links B", projectB);
  const workspaces = await api.workspaces();
  const workspaceA = workspaces.find((w) => w.cwd === projectA);
  const workspaceB = workspaces.find((w) => w.cwd === projectB);
  if (!workspaceA || !workspaceB) throw new Error("expected both workspaces to exist");

  const created = await api.post<CreatePlanResponse>("/plans", {
    title: "Owner workspace plan",
    project_path: projectB,
    owner: { pane_id: "chat-plan-orchestrator", session_id: null, title: "Chat plan orchestrator" },
    kinds: "progress:#38bdf8",
  });
  const section = await api.post<CreateSectionResponse>(`/plans/${created.plan_id}/sections`, {
    title: "Cross workspace section",
    body: "Section body",
  });
  await api.post(`/plans/${created.plan_id}/mode`, { mode: "approved" });
  const task = await api.post<CreateTaskResponse>(`/plans/${created.plan_id}/tasks`, {
    name: "Cross workspace task",
    type: "research",
    sections: section.section_id,
    body: "Task body",
    parent: null,
  });
  await expect
    .poll(async () => {
      const body = await api.get<{ plans: Array<{ plan_id: string }> }>("/plans");
      return body.plans.some((plan) => plan.plan_id === created.plan_id);
    }, { timeout: 30_000 })
    .toBe(true);

  return { workspaceA, workspaceB, planId: created.plan_id, taskId: task.task_id };
}

async function pushAssistantMessage(page: Page, paneId: string, content: string, id: string) {
  await page.evaluate(
    ({ paneId: targetPaneId, content: messageContent, id: messageId }) => {
      const testChat = window.__codetermTestChat;
      if (!testChat) throw new Error("__codetermTestChat is not installed");
      testChat.pushMessages(targetPaneId, [{
        id: messageId,
        type: "assistant",
        content: messageContent,
        timestamp: String(Date.now()),
        provider: "test",
      }]);
    },
    { paneId, content, id },
  );
}

test("@golden chat-task-link cross-workspace Enter opens plan-view-shell and focuses task", async ({ api, ui, paneCleanup }) => {
  test.setTimeout(180_000);
  await ui.reconnect();
  const hasTestHook = await ui.page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "__codetermTestChat not available in this build");

  const { workspaceA, workspaceB, planId, taskId } = await seedCrossWorkspacePlan(api);
  await api.focusWorkspace(workspaceA.id);
  const paneId = await openMockChatPane(api, paneCleanup, ui.page, workspaceA.id);

  await pushAssistantMessage(ui.page, paneId, `Investigate ${planId} then complete ${taskId}`, "pw-plan-link-task");
  const chat = ui.page.locator(`[data-testid="pane-session-chat"][data-pane-id="${paneId}"]`);
  const planLink = chat.locator(`[data-testid="chat-plan-link"][data-plan-id="${planId}"]`).first();
  const taskLink = chat.locator(`[data-testid="chat-task-link"][data-plan-id="${planId}"][data-target-id="${taskId}"]`).first();
  await expect(planLink).toBeVisible({ timeout: 30_000 });
  await expect(taskLink).toBeVisible({ timeout: 30_000 });
  await expect(planLink).toContainText("Owner workspace plan", { timeout: 30_000 });
  await expect(taskLink).toContainText("Cross workspace task", { timeout: 30_000 });
  await expect(taskLink).toHaveAttribute("title", new RegExp(`${planId}/${taskId}`));

  await ui.page.evaluate(() => {
    window.__ctCopiedPlanRef = "";
    const original = navigator.clipboard.writeText.bind(navigator.clipboard);
    navigator.clipboard.writeText = async (text: string) => {
      window.__ctCopiedPlanRef = text;
      return original(text);
    };
  });
  await taskLink.dispatchEvent("contextmenu");
  await expect.poll(async () => ui.page.evaluate(() => window.__ctCopiedPlanRef)).toBe(`${planId}/${taskId}`);

  await taskLink.focus();
  await ui.page.keyboard.press("Enter");
  await expect
    .poll(async () => (await api.workspaces()).find((workspace) => workspace.id === workspaceB.id)?.is_active ?? false, { timeout: 30_000 })
    .toBe(true);
  await expect(ui.page.locator('[data-testid="plan-view-shell"]').first()).toBeVisible({ timeout: 30_000 });
  await expect(ui.page.locator('[data-testid="plan-board-task-title"]').filter({ hasText: "Cross workspace task" }).first()).toBeVisible({ timeout: 30_000 });

  const missingPlanId = "pl_missingplan";
  await api.focusWorkspace(workspaceA.id);
  await ui.page.reload({ waitUntil: "domcontentloaded" });
  await pushAssistantMessage(ui.page, paneId, `Missing ref ${missingPlanId}`, "pw-plan-link-missing");
  const missingLink = chat.locator('[data-testid="chat-plan-link"]').filter({ hasText: missingPlanId }).first();
  await expect(missingLink).toBeVisible({ timeout: 30_000 });
  await expect(missingLink).toHaveAttribute("data-ref-state", "missing");
  await expect(missingLink).toBeDisabled();
  await expect
    .poll(async () => (await api.workspaces()).find((workspace) => workspace.id === workspaceA.id)?.is_active ?? false, { timeout: 10_000 })
    .toBe(true);
  await expect(ui.page.locator('[data-testid="plan-view-shell"]').first()).toHaveCount(0);
});

test("@golden chat-plan-link mobile Space switches workspace and opens plan-view-shell", async ({ api, ui, paneCleanup }) => {
  test.setTimeout(180_000);
  await ui.reconnect();
  const hasTestHook = await ui.page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "__codetermTestChat not available in this build");

  const { workspaceA, workspaceB, planId } = await seedCrossWorkspacePlan(api);
  await api.focusWorkspace(workspaceA.id);
  await ui.page.setViewportSize({ width: 390, height: 844 });
  const paneId = await openMockChatPane(api, paneCleanup, ui.page, workspaceA.id);

  await pushAssistantMessage(ui.page, paneId, `Open ${planId}`, "pw-plan-link-plan");
  const chat = ui.page.locator(`[data-testid="pane-session-chat"][data-pane-id="${paneId}"]`);
  const planLink = chat.locator(`[data-testid="chat-plan-link"][data-plan-id="${planId}"]`).first();
  await expect(planLink).toBeVisible({ timeout: 30_000 });
  await expect(planLink).toContainText("Owner workspace plan", { timeout: 30_000 });
  await planLink.focus();
  await ui.page.keyboard.press("Space");

  await expect
    .poll(async () => (await api.workspaces()).find((workspace) => workspace.id === workspaceB.id)?.is_active ?? false, { timeout: 30_000 })
    .toBe(true);
  await expect(ui.page.locator('[data-testid="plan-view-shell"]').first()).toBeVisible({ timeout: 30_000 });
});
