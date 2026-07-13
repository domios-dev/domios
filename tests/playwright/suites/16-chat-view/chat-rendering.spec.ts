/**
 * WHAT: Chat view renders empty, user, assistant, code, and image message shapes.
 * WHY: Provider transcript parsing must remain readable across structured chat UI changes.
 * ORACLE: Seeded transcript records render as the expected message blocks and attachment previews.
 */
import { expect, test, type PaneCleanup } from "../../helpers/fixtures";
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
      pushMessages: (paneId: string, messages: TestChatMessage[]) => void;
      resetClock: () => void;
    };
  }
}

const PNG_1X1_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

async function openMockChatPane(
  api: ApiClient,
  paneCleanup: PaneCleanup,
  page: import("../../helpers/fixtures").Page,
): Promise<string> {
  const pane = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
    title: `pw-chat-render-${Date.now()}`,
  });
  await api.post(`/panes/${pane.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/panes/${pane.id}/chat-view`, { enabled: true });
  await api.focusPane(pane.id);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  if (pane.tab_id) {
    const tab = page.locator(`[data-testid="tab-item"][data-tab-id="${pane.tab_id}"]`).first();
    await expect(tab).toBeVisible({ timeout: 30_000 });
    await tab.click();
  }

  const chat = page.locator(`[data-testid="pane-session-chat"][data-pane-id="${pane.id}"]`);
  await expect(chat).toBeVisible({ timeout: 30_000 });
  await expect(chat.locator('[data-testid="chat-empty-state"], [data-testid="chat-recovering-state"]')).toBeVisible({ timeout: 30_000 });
  return pane.id;
}

async function scrollChatAwayFromBottom(
  page: import("../../helpers/fixtures").Page,
  paneId: string,
  scrollContainer: ReturnType<import("../../helpers/fixtures").Page["locator"]>,
) {
  const overflow = await scrollContainer.evaluate((el) => el.scrollHeight - el.clientHeight);
  if (overflow <= 320) {
    await page.evaluate(
      ({ paneId: targetPaneId }) => {
        window.__codetermTestChat?.pushMessages(targetPaneId, Array.from({ length: 80 }, (_, i) => ({
          id: `scroll-overflow-primer-${i + 1}`,
          type: "assistant",
          content: `scroll primer ${i + 1}`,
          timestamp: String(1700003000006 + i),
          provider: "test",
        })));
      },
      { paneId },
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

test("@smoke ChatView PTY provider pane shows no inline option card without a pending_prompt", async ({
  api,
  ui,
}) => {
  // leave the worker-scoped session in a broken state after a dev-server restart.
  await ui.reconnect();

  // Find an existing terminal pane that is already visible in the web mirror.
  const panes = await api.panes();
  const target = panes.find((p) => p.focused) ?? panes[0];
  expect(target, "no pane found").toBeTruthy();
  await api.post(`/panes/${target.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/panes/${target.id}/chat-view`, { enabled: true });

  // The React app receives the SSE event and renders pane-session-chat.
  const chat = ui.page.locator(`[data-testid="pane-session-chat"]`).first();
  await expect(chat).toBeVisible({ timeout: 30_000 });

  // is explicitly set via hook — card_frames=0 invariant from detection replays.
  await expect(ui.page.locator('[data-testid="pane-pending-prompt"]')).toHaveCount(0);
});

test("@golden ChatView message DOM order matches chronological push order", async ({
  api,
  ui,
  paneCleanup,
}) => {
  await ui.reconnect();
  const paneId = await openMockChatPane(api, paneCleanup, ui.page);
  const hasTestHook = await ui.page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "window.__codetermTestChat not available in this build (macOS web mirror without VITE_CODETERM_TEST=1 build)");
  const messages: TestChatMessage[] = [
    { id: "pw-order-1", type: "user", content: "pw-order-first", timestamp: "1700002000001", provider: "test" },
    { id: "pw-order-2", type: "assistant", content: "pw-order-second", timestamp: "1700002000002", provider: "test" },
    { id: "pw-order-3", type: "user", content: "pw-order-third", timestamp: "1700002000003", provider: "test" },
  ];

  await ui.page.evaluate(
    ({ paneId: targetPaneId, messages: targetMessages }) => {
      const testChat = window.__codetermTestChat;
      if (!testChat) throw new Error("window.__codetermTestChat is not installed");
      testChat.pushMessages(targetPaneId, targetMessages);
    },
    { paneId, messages },
  );

  const chatMessages = ui.page.locator(
    `[data-testid="pane-session-chat"][data-pane-id="${paneId}"] [data-testid="pane-session-chat-messages"] [data-testid="chat-message"]`,
  );
  await expect(chatMessages).toHaveCount(3, { timeout: 10_000 });

  const texts = await chatMessages.evaluateAll((els) =>
    els.map((el) => (el as HTMLElement).innerText?.trim() ?? ""),
  );
  expect(texts[0]).toContain("pw-order-first");
  expect(texts[1]).toContain("pw-order-second");
  expect(texts[2]).toContain("pw-order-third");
  await expect(ui.page.locator(`[data-testid="pane-session-chat"][data-pane-id="${paneId}"] [data-testid="chat-empty-state"]`)).toHaveCount(0);
});

test("@golden ChatView transcript remains correct across tool results, focus, and scroll state", async ({
  api,
  ui,
  paneCleanup,
}) => {
  const hasTestHook = await ui.page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "window.__codetermTestChat not available in this build (macOS web mirror without VITE_CODETERM_TEST=1 build)");

  await ui.reconnect();
  const paneId = await openMockChatPane(api, paneCleanup, ui.page);
  const chat = ui.page.locator(`[data-testid="pane-session-chat"][data-pane-id="${paneId}"]`);
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
    ({ paneId: targetPaneId, messages: targetMessages }) => {
      const testChat = window.__codetermTestChat;
      if (!testChat) throw new Error("window.__codetermTestChat is not installed");
      testChat.pushMessages(targetPaneId, targetMessages);
    },
    { paneId, messages },
  );

  await expect(composer).toBeFocused();
  await expect(chat.locator('[data-testid="chat-empty-state"]')).toHaveCount(0);

  const chatMessages = chat.locator('[data-testid="pane-session-chat-messages"] [data-testid="chat-message"]');
  await expect(chatMessages).toHaveCount(4, { timeout: 10_000 });
  const texts = await chatMessages.evaluateAll((els) => els.map((el) => (el as HTMLElement).innerText));
  expect(texts.join("\n")).toContain("run a command");
  expect(texts.join("\n")).toContain("ok");
  expect(texts.join("\n")).toContain("latest failure");

  await expect(chat.locator('[data-testid="chat-tool-call"]')).toHaveCount(3);
  await expect(chat.locator('[data-testid="chat-message"][data-message-role="tool_result"]')).toHaveCount(0);

  const scrollContainer = chat.locator('[data-testid="pane-session-chat-messages"]');
  await scrollChatAwayFromBottom(ui.page, paneId, scrollContainer);
  await ui.page.evaluate(
    ({ paneId: targetPaneId }) => {
      window.__codetermTestChat?.pushMessages(targetPaneId, [{
        id: "scroll-preserve-1",
        type: "assistant",
        content: "new message after manual scroll",
        timestamp: "1700003000087",
        provider: "test",
      }]);
    },
    { paneId },
  );
  await expect(chat.locator('[data-testid="chat-scroll-bottom"]')).toBeVisible({ timeout: 10_000 });
  await expect(composer).toBeFocused();
});

test("@golden ChatView renders message blocks, code blocks, and image attachments from mock replies", async ({
  api,
  ui,
  paneCleanup,
}) => {
  const hasTestHook = await ui.page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "window.__codetermTestChat not available in this build (macOS web mirror without VITE_CODETERM_TEST=1 build)");

  const paneId = await openMockChatPane(api, paneCleanup, ui.page);
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
    ({ paneId: targetPaneId, messages: targetMessages }) => {
      const testChat = window.__codetermTestChat;
      if (!testChat) throw new Error("window.__codetermTestChat is not installed");
      testChat.pushMessages(targetPaneId, targetMessages);
    },
    { paneId, messages },
  );

  const chat = ui.page.locator(`[data-testid="pane-session-chat"][data-pane-id="${paneId}"]`);
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
