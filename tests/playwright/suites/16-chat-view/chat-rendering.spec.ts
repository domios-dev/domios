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
  const pane = await paneCleanup.createPane<{ id: string }>({ title: `pw-chat-render-${Date.now()}` });
  await api.post(`/panes/${pane.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/panes/${pane.id}/chat-view`, { enabled: true });
  await api.focusPane(pane.id);

  const tabId = ((await api.panes()).find((p) => p.id === pane.id) as { tab_id?: string } | undefined)?.tab_id;
  if (tabId) {
    await page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`).click();
  }

  const chat = page.locator(`[data-pane-id="${pane.id}"] [data-testid="pane-session-chat"]`);
  await expect(chat).toBeVisible({ timeout: 30_000 });
  await expect(chat.locator('[data-testid="chat-empty-state"]')).toBeVisible({ timeout: 30_000 });
  return pane.id;
}

test("@golden ChatView renders message blocks, code blocks, and image attachments from mock replies", async ({
  api,
  ui,
  paneCleanup,
}) => {
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

  const chat = ui.page.locator(`[data-pane-id="${paneId}"] [data-testid="pane-session-chat"]`);
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
