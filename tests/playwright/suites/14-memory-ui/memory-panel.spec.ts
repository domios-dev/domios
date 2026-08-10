/**
 * WHAT: Memory panel supports list, detail, edit, and related-item navigation.
 * WHY: Mem UI changes must preserve the operator workflow for inspecting and maintaining saved facts.
 * ORACLE: Created mem items appear in UI, open detail/editor surfaces, and related rows resolve to real entities.
 */
import { expect, test } from "../../helpers/fixtures";

interface MemItem {
  id: string;
  title: string;
  body: string;
  kind: string;
}

const created: string[] = [];

async function memSave(
  api: { post<T>(p: string, body?: unknown): Promise<T> },
  kind: string,
  title: string,
  body: string,
): Promise<string> {
  const res = await api.post<{ id: string }>("/mem/items", { kind, title, body });
  expect(res.id).toBeTruthy();
  created.push(res.id);
  return res.id;
}

test.afterEach(async ({ api, ui }) => {
  await ui.page.locator('[data-testid="memory-item-close"]').click({ timeout: 1_000 }).catch(() => {});
  for (const id of created) {
    await api.request("DELETE", `/mem/items/${id}`).catch(() => {});
  }
  created.length = 0;
});

async function openMemoryList(page: import("../../helpers/fixtures").Page): Promise<void> {
  await page.locator('[data-testid="nav-memory"]').click();
  await expect(page.locator('[data-testid="memory-panel"]')).toBeVisible();
  await page.locator('[data-testid="memory-tab-inspect"]').click();
  const toggle = page.locator('[data-testid="memory-inspect-view-toggle"]');
  await expect(toggle).toBeVisible();
  if ((await toggle.getAttribute("data-memory-inspect-view")) === "graph") {
    await toggle.click();
  }
  await expect(page.locator('[data-testid="memory-list-tab"]')).toBeVisible();
}

async function waitForGraphSnapshot(
  api: { get<T>(p: string): Promise<T>; post<T = unknown>(p: string, body?: unknown): Promise<T> },
  ids: string[],
): Promise<void> {
  await api.post("/mem/graph/layout/rebuild").catch((error) => {
    if (!String(error).includes("409")) throw error;
  });
  await expect.poll(async () => {
    const snap = await api.get<{ nodes?: Array<{ id?: string }> }>("/mem/graph/snapshot");
    const present = new Set((snap.nodes ?? []).map((node) => node.id).filter(Boolean));
    return ids.every((id) => present.has(id));
  }, { timeout: 30_000, intervals: [250, 500, 1_000] }).toBe(true);
}

test("@golden MemoryPanel searches saved memory and opens item preview", async ({ api, ui }) => {
  const marker = `pw-memory-ui-search-${Date.now()}`;
  const id = await memSave(
    api,
    "fact",
    `Memory UI search probe ${marker}`,
    `Preview body for ${marker}: the panel should show this exact text.`,
  );
  const item = await api.get<MemItem>(`/mem/items/${id}`);
  expect(item.title).toContain(marker);
  // MemoryListTab intersects search hits with the graph snapshot it was handed,
  // so an item absent from that snapshot can never list however it scores.
  await waitForGraphSnapshot(api, [id]);

  await openMemoryList(ui.page);
  await ui.page.locator('[data-testid="memory-list-search"]').fill(marker);

  const row = ui.page.locator(`[data-testid="memory-list-item"][data-memory-id="${id}"]`);
  await expect(row).toBeVisible({ timeout: 20_000 });
  await expect(row).toContainText(marker);
  await row.click();

  const editor = ui.page.locator(`[data-testid="memory-item-editor"][data-memory-id="${id}"]`);
  await expect(editor).toBeVisible();
  await expect(editor.locator('[data-testid="memory-item-title-input"]')).toHaveValue(item.title);
  await expect(editor.locator('[data-testid="memory-item-body-input"]')).toHaveValue(item.body);
});

test("@golden MemoryPanel renders graph relations in item detail", async ({ api, ui }) => {
  const stamp = Date.now();
  const a = await memSave(
    api,
    "fact",
    `Memory relation alpha ${stamp}`,
    `Alpha relation body ${stamp}`,
  );
  const b = await memSave(
    api,
    "fact",
    `Memory relation beta ${stamp}`,
    `Beta relation body ${stamp}`,
  );
  await api.post("/mem/graph/relate", { from: a, rel: "relates_to", to: b });
  await waitForGraphSnapshot(api, [a, b]);

  await openMemoryList(ui.page);
  await ui.page.locator('[data-testid="memory-list-search"]').fill(`Memory relation alpha ${stamp}`);
  const row = ui.page.locator(`[data-testid="memory-list-item"][data-memory-id="${a}"]`);
  await expect(row).toBeVisible({ timeout: 20_000 });
  await row.locator('[data-testid="memory-list-view-graph"]').click();

  await expect(ui.page.locator('[data-testid="memory-panel"]')).toBeVisible();
  const detail = ui.page.locator(`[data-testid="memory-node-detail"][data-memory-id="${a}"]`);
  await expect(detail).toBeVisible({ timeout: 30_000 });
  await expect(detail.locator('[data-testid="memory-node-body-preview"]')).toContainText(
    `Alpha relation body ${stamp}`,
  );

  const related = detail.locator(`[data-testid="memory-node-related-item"][data-memory-id="${b}"]`);
  await expect(related).toBeVisible({ timeout: 30_000 });
  await expect(related).toHaveAttribute("data-memory-rel", "relates_to");
  await expect(related).toContainText(`Memory relation beta ${stamp}`);
});
