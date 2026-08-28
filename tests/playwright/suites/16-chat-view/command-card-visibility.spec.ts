/**
 * WHAT: A person can reveal the complete command behind a compact chat card without widening the conversation.
 * WHY: A truncated command can hide the exact operation that ran, while an unbounded command can make the chat unreadable.
 * ORACLE: The named disclosure reveals the exact seeded command and its rendered command region scrolls inside the card.
 */
import { expect, test, type Page, type TabCleanup } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";

interface TestChatMessage {
  id: string;
  type: "tool_call";
  content: string;
  timestamp: string;
  provider: string;
  toolName: string;
  toolInput: { command: string };
  toolCallId: string;
}

async function openChatTab(api: ApiClient, tabCleanup: TabCleanup, page: Page): Promise<string> {
  const tab = await tabCleanup.createTab<{ id: string }>({ title: `pw-command-card-${Date.now()}` });
  await api.post(`/tabs/${tab.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/tabs/${tab.id}/chat-view`, { enabled: true });
  await api.focusTab(tab.id);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  const row = page.locator(`[data-testid="tab-item"][data-tab-id="${tab.id}"]`).first();
  await expect(row).toBeVisible({ timeout: 30_000 });
  await row.click();
  await expect(page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tab.id}"]`)).toBeVisible({ timeout: 30_000 });
  return tab.id;
}

test("@golden @wave-1-10-3 command card reveals the full command inside a scrolling region", async ({
  api,
  ui,
  tabCleanup,
}) => {
  const tabId = await openChatTab(api, tabCleanup, ui.page);
  const fullCommand = Array.from({ length: 80 }, (_, index) => `printf command-${index + 1}`).join("\n");
  await expect.poll(() => ui.page.evaluate(() => typeof (window as typeof window & { __codetermTestChat?: unknown }).__codetermTestChat)).toBe("object");

  await ui.page.evaluate(
    ({ targetTabId, command }) => {
      const testChat = (window as typeof window & {
        __codetermTestChat?: { pushMessages: (tabId: string, messages: TestChatMessage[]) => void };
      }).__codetermTestChat;
      if (!testChat) throw new Error("window.__codetermTestChat is not installed");
      testChat.pushMessages(targetTabId, [{
        id: "pw-long-command",
        type: "tool_call",
        content: "command",
        timestamp: "1700005000000",
        provider: "test",
        toolName: "shell",
        toolInput: { command },
        toolCallId: "pw-long-command-call",
      }]);
    },
    { targetTabId: tabId, command: fullCommand },
  );

  const chat = ui.page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"]`);
  const card = chat.locator('[data-testid="chat-tool-call"]');
  await expect(card).toHaveCount(1);
  const disclosure = card.getByRole("button", { name: "Show full command" });
  await expect(disclosure).toBeVisible();
  await expect(card.locator('[data-testid="toolcard-command"]')).toHaveCount(0);
  await disclosure.click();

  const command = card.getByRole("region", { name: "Full command text" });
  await expect(command).toHaveText(fullCommand);
  const collapse = card.getByRole("button", { name: "Hide full command" });
  await expect(collapse).toHaveAttribute("aria-expanded", "true");
  await expect(collapse).toBeFocused();
  await expect(command).toHaveCSS("overflow", "auto");
  await expect.poll(() => command.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
  await ui.page.keyboard.press("Tab");
  await expect(command).toBeFocused();
  await ui.page.keyboard.press("End");
  await expect.poll(() => command.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  await expect.poll(() => card.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
});
