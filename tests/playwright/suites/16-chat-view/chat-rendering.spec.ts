/**
 * WHAT: Chat view renders empty, user, assistant, code, and image message shapes.
 * WHY: Provider transcript parsing must remain readable across structured chat UI changes.
 * ORACLE: Seeded transcript records render as the expected message blocks and attachment previews.
 */
import { expect, test, type TabCleanup } from "../../helpers/fixtures";
import { type ApiClient } from "../../helpers/api-client";

type ChatMessageKind = "user" | "assistant" | "system" | "image" | "tool_call" | "tool_result";

interface TestChatAttachment {
  id: string;
  mimeType: string;
  data?: string;
}

interface TestChatMessage {
  id: string;
  type: ChatMessageKind;
  content: string;
  timestamp: string;
  provider?: string;
  attachments?: TestChatAttachment[];
  toolName?: string;
  toolInput?: unknown;
  toolResult?: string;
  toolCallId?: string;
  toolError?: boolean;
}

declare global {
  interface Window {
    __codetermTestChat?: {
      pushMessages: (tabId: string, messages: TestChatMessage[]) => void;
      resetClock: () => void;
    };
  }
}

const PNG_1X1_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

async function openMockChatTab(
  api: ApiClient,
  tabCleanup: TabCleanup,
  page: import("../../helpers/fixtures").Page,
): Promise<string> {
  const tab = await tabCleanup.createTab<{ id: string; tab_id?: string }>({
    title: `pw-chat-render-${Date.now()}`,
  });
  await api.post(`/tabs/${tab.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/tabs/${tab.id}/chat-view`, { enabled: true });
  await api.focusTab(tab.id);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  const row = page.locator(`[data-testid="tab-item"][data-tab-id="${tab.id}"]`).first();
  await expect(row).toBeVisible({ timeout: 30_000 });
  await row.click();

  const chat = page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tab.id}"]`);
  await expect(chat).toBeVisible({ timeout: 30_000 });
  await expect(chat.locator('[data-testid="chat-empty-state"], [data-testid="chat-recovering-state"]')).toBeVisible({ timeout: 30_000 });
  return tab.id;
}

async function scrollChatAwayFromBottom(
  page: import("../../helpers/fixtures").Page,
  tabId: string,
  scrollContainer: ReturnType<import("../../helpers/fixtures").Page["locator"]>,
) {
  const overflow = await scrollContainer.evaluate((el) => el.scrollHeight - el.clientHeight);
  if (overflow <= 320) {
    await page.evaluate(
      ({ tabId: targetTabId }) => {
        window.__codetermTestChat?.pushMessages(targetTabId, Array.from({ length: 80 }, (_, i) => ({
          id: `scroll-overflow-primer-${i + 1}`,
          type: "assistant",
          content: `scroll primer ${i + 1}`,
          timestamp: String(1700003000006 + i),
          provider: "test",
        })));
      },
      { tabId },
    );
    await expect(
      scrollContainer.locator('[data-testid="chat-message"]').filter({ hasText: "scroll primer 80" }),
    ).toBeVisible({ timeout: 10_000 });
  }

  await expect.poll(
    () => scrollContainer.evaluate((el) => el.scrollHeight - el.clientHeight),
    { timeout: 10_000 },
  ).toBeGreaterThan(320);

  await scrollContainer.evaluate((el) => {
    el.scrollTop = 0;
    el.dispatchEvent(new Event("scroll", { bubbles: true }));
  });

  await expect.poll(
    () => scrollContainer.evaluate((el) => el.scrollHeight - el.scrollTop - el.clientHeight),
    { timeout: 10_000 },
  ).toBeGreaterThan(240);
}

test("@smoke ChatView PTY provider tab shows no inline option card without a pending_prompt", async ({
  api,
  ui,
}) => {
  // leave the worker-scoped session in a broken state after a dev-server restart.
  await ui.reconnect();

  // Find an existing terminal tab that is already visible in the web mirror.
  const tabs = await api.tabs();
  const target = tabs.find((p) => p.focused) ?? tabs[0];
  expect(target, "no tab found").toBeTruthy();
  await api.post(`/tabs/${target.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/tabs/${target.id}/chat-view`, { enabled: true });

  // The React app receives the SSE event and renders tab-session-chat.
  const chat = ui.page.locator(`[data-testid="tab-session-chat"]`).first();
  await expect(chat).toBeVisible({ timeout: 30_000 });

  // is explicitly set via hook — card_frames=0 invariant from detection replays.
  await expect(ui.page.locator('[data-testid="tab-pending-prompt"]')).toHaveCount(0);
});

test("@golden ChatView message DOM order matches chronological push order", async ({
  api,
  ui,
  tabCleanup,
}) => {
  await ui.reconnect();
  const tabId = await openMockChatTab(api, tabCleanup, ui.page);
  const hasTestHook = await ui.page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "window.__codetermTestChat not available in this build (macOS web mirror without VITE_CODETERM_TEST=1 build)");
  const messages: TestChatMessage[] = [
    { id: "pw-order-1", type: "user", content: "pw-order-first", timestamp: "1700002000001", provider: "test" },
    { id: "pw-order-2", type: "assistant", content: "pw-order-second", timestamp: "1700002000002", provider: "test" },
    { id: "pw-order-3", type: "user", content: "pw-order-third", timestamp: "1700002000003", provider: "test" },
  ];

  await ui.page.evaluate(
    ({ tabId: targetTabId, messages: targetMessages }) => {
      const testChat = window.__codetermTestChat;
      if (!testChat) throw new Error("window.__codetermTestChat is not installed");
      testChat.pushMessages(targetTabId, targetMessages);
    },
    { tabId, messages },
  );

  const chatMessages = ui.page.locator(
    `[data-testid="tab-session-chat"][data-tab-id="${tabId}"] [data-testid="tab-session-chat-messages"] [data-testid="chat-message"]`,
  );
  await expect(chatMessages).toHaveCount(3, { timeout: 10_000 });

  const texts = await chatMessages.evaluateAll((els) =>
    els.map((el) => (el as HTMLElement).innerText?.trim() ?? ""),
  );
  expect(texts[0]).toContain("pw-order-first");
  expect(texts[1]).toContain("pw-order-second");
  expect(texts[2]).toContain("pw-order-third");
  await expect(ui.page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"] [data-testid="chat-empty-state"]`)).toHaveCount(0);
});

test("@golden ChatView transcript remains correct across tool results, focus, and scroll state", async ({
  api,
  ui,
  tabCleanup,
}) => {
  const hasTestHook = await ui.page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "window.__codetermTestChat not available in this build (macOS web mirror without VITE_CODETERM_TEST=1 build)");

  await ui.reconnect();
  const tabId = await openMockChatTab(api, tabCleanup, ui.page);
  const chat = ui.page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"]`);
  const composer = chat.locator('[data-testid="chat-composer"]');
  await composer.focus();

  const messages: TestChatMessage[] = [
    { id: "tool-user-1", type: "user", content: "run a command", timestamp: "1700003000001", provider: "test" },
    {
      id: "tool-call-1",
      type: "tool_call",
      content: "exec",
      timestamp: "1700003000002",
      provider: "test",
      toolName: "shell",
      toolInput: { command: "echo ok" },
      toolCallId: "call-1",
    },
    {
      id: "tool-result-1",
      type: "tool_result",
      content: "ok",
      timestamp: "1700003000003",
      provider: "test",
      toolCallId: "call-1",
      toolResult: "ok",
    },
    {
      id: "tool-call-failed-old",
      type: "tool_call",
      content: "old failure",
      timestamp: "1700003000004",
      provider: "test",
      toolName: "shell",
      toolInput: { command: "exit 1" },
      toolCallId: "call-old",
      toolError: true,
    },
    {
      id: "tool-call-failed-latest",
      type: "tool_call",
      content: "latest failure",
      timestamp: "1700003000005",
      provider: "test",
      toolName: "shell",
      toolInput: { command: "exit 2" },
      toolCallId: "call-latest",
      toolError: true,
    },
  ];

  await ui.page.evaluate(
    ({ tabId: targetTabId, messages: targetMessages }) => {
      const testChat = window.__codetermTestChat;
      if (!testChat) throw new Error("window.__codetermTestChat is not installed");
      testChat.pushMessages(targetTabId, targetMessages);
    },
    { tabId, messages },
  );

  await expect(composer).toBeFocused();
  await expect(chat.locator('[data-testid="chat-empty-state"]')).toHaveCount(0);

  const chatMessages = chat.locator('[data-testid="tab-session-chat-messages"] [data-testid="chat-message"]');
  await expect(chatMessages).toHaveCount(4, { timeout: 10_000 });
  const texts = await chatMessages.evaluateAll((els) => els.map((el) => (el as HTMLElement).innerText));
  expect(texts.join("\n")).toContain("run a command");
  expect(texts.join("\n")).toContain("ok");
  expect(texts.join("\n")).toContain("latest failure");

  await expect(chat.locator('[data-testid="chat-tool-call"]')).toHaveCount(3);
  await expect(chat.locator('[data-testid="chat-message"][data-message-role="tool_result"]')).toHaveCount(0);

  const scrollContainer = chat.locator('[data-testid="tab-session-chat-messages"]');
  await scrollChatAwayFromBottom(ui.page, tabId, scrollContainer);
  await ui.page.evaluate(
    ({ tabId: targetTabId }) => {
      window.__codetermTestChat?.pushMessages(targetTabId, [{
        id: "scroll-preserve-1",
        type: "assistant",
        content: "new message after manual scroll",
        timestamp: "1700003000087",
        provider: "test",
      }]);
    },
    { tabId },
  );
  await expect(chat.locator('[data-testid="chat-scroll-bottom"]')).toBeVisible({ timeout: 10_000 });
  await expect(composer).toBeFocused();
});

test("@golden ChatView renders message blocks, code blocks, and image attachments from mock replies", async ({
  api,
  ui,
  tabCleanup,
}) => {
  const hasTestHook = await ui.page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "window.__codetermTestChat not available in this build (macOS web mirror without VITE_CODETERM_TEST=1 build)");

  const tabId = await openMockChatTab(api, tabCleanup, ui.page);
  const messages: TestChatMessage[] = [
    {
      id: "pw-chat-user-block",
      type: "user",
      content: "Render this user message block.",
      timestamp: "1700000000001",
      provider: "test",
    },
    {
      id: "pw-chat-assistant-code",
      type: "assistant",
      content: [
        "Here is a TypeScript block:",
        "",
        "```ts",
        "const answer: number = 42;",
        "export const label = `answer-${answer}`;",
        "```",
      ].join("\n"),
      timestamp: "1700000000002",
      provider: "test",
    },
    {
      id: "pw-chat-user-image",
      type: "user",
      content: "Screenshot attached for renderer coverage.",
      timestamp: "1700000000003",
      provider: "test",
      attachments: [{ id: "pw-image", mimeType: "image/png", data: PNG_1X1_BASE64 }],
    },
  ];

  await ui.page.evaluate(
    ({ tabId: targetTabId, messages: targetMessages }) => {
      const testChat = window.__codetermTestChat;
      if (!testChat) throw new Error("window.__codetermTestChat is not installed");
      testChat.pushMessages(targetTabId, targetMessages);
    },
    { tabId, messages },
  );

  const chat = ui.page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"]`);
  const userBlock = chat
    .locator('[data-testid="chat-message"][data-message-role="user"]')
    .filter({ hasText: "Render this user message block." });
  await expect(userBlock).toBeVisible();

  const codeBlock = chat.locator('[data-testid="chat-code-block"][data-code-language="ts"]');
  await expect(codeBlock).toBeVisible();
  await expect(codeBlock).toContainText("const answer: number = 42");

  const imageBlock = chat
    .locator('[data-testid="chat-message"][data-message-role="user"]')
    .filter({ hasText: "Screenshot attached for renderer coverage." });
  await expect(imageBlock).toBeVisible();
  await imageBlock.locator('[data-testid="chat-image-attachment-button"]').click();

  const imagePreview = imageBlock.locator('[data-testid="chat-image-preview"] img');
  await expect(imagePreview).toBeVisible();
  await expect(imagePreview).toHaveAttribute("src", /^data:image\/png;base64,/);
});
