import { expect, test, type PaneCleanup } from "../../helpers/fixtures";
import { type ApiClient } from "../../helpers/api-client";

type ChatMessageKind = "user" | "assistant" | "system" | "image";

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
  await expect(chat.locator('[data-testid="chat-empty-state"]')).toBeVisible({ timeout: 30_000 });
  return pane.id;
}

test("@smoke ChatView PTY provider pane shows no inline option card without a pending_prompt", async ({
  api,
  ui,
}) => {
  // Reconnect to get a fresh browser session — previous suites (extensions) can
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

  // PTY providers must never show an inline option card unless a pending_prompt
  // is explicitly set via hook — card_frames=0 invariant from detection replays.
  await expect(ui.page.locator('[data-testid="pane-pending-prompt"]')).toHaveCount(0);
});

test("@golden ChatView message DOM order matches chronological push order", async ({
  api,
  ui,
  paneCleanup,
}) => {
  // window.__codetermTestChat is injected by VITE_CODETERM_TEST=1 (dev-build
  // only). On macOS the web mirror serves dist/ without the test flag; skip
  const hasTestHook = await ui.page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "window.__codetermTestChat not available in this build (macOS web mirror without VITE_CODETERM_TEST=1 build)");

  await ui.reconnect();
  const paneId = await openMockChatPane(api, paneCleanup, ui.page);
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
