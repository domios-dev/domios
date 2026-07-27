/**
 * WHAT: Open-shell LM Studio flow exposes chatBackend setup and launch surfaces.
 * WHY: Local chat backends are non-PTY providers and need coverage separate from terminal agents.
 * ORACLE: Plugin/provider setup, browser UI state, and spawned tab metadata match the LM Studio path.
 */
import { execFileSync } from "node:child_process";
import path from "node:path";
import {
  API_BASE,
  expect,
  readDevToken,
  test,
  type Browser,
  type Page,
  type TabCleanup,
} from "../../helpers/fixtures";
import { devDataDir } from "../../helpers/reset";
import { REPO_ROOT } from "../../helpers/dev-console";
import { type ApiClient } from "../../helpers/api-client";

const WEB_URL = API_BASE + "/";
const PLUGIN_ID = "lmstudio";
const SYSTEM_PROMPT_PREFIX = "-=-codeterm:system_prompt-=-";

type ChatMessageKind = "user" | "assistant" | "tool_call" | "tool_result" | "system";

interface TestChatMessage {
  id: string;
  type: ChatMessageKind;
  content: string;
  timestamp: string;
  provider?: string;
  toolId?: string;
  toolName?: string;
  toolArgs?: string;
  toolResult?: string;
}

interface OpenShellTab {
  id: string;
  tab_id: string;
  tab_id: string;
  pty_id: string | null;
}

declare global {
  interface Window {
    __codetermTestChat?: {
      pushMessages: (tabId: string, messages: TestChatMessage[]) => void;
      resetClock: () => void;
    };
  }
}

async function newWebPage(browser: Browser): Promise<Page> {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  await page.goto(WEB_URL + "#token=" + encodeURIComponent(readDevToken()), {
    waitUntil: "domcontentloaded",
  });
  await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 30_000 });
  return page;
}

async function expectLmStudioShellProvider(api: ApiClient): Promise<void> {
  const providers = await api.get<Array<{ id: string; kind?: string; supports_chat_ui?: boolean }>>(
    "/providers",
  );
  const shell = providers.find((provider) => provider.id === PLUGIN_ID);
  // lmstudio is optional marketplace plugin state.
  // Skip like 04-mesh does for mesh.enabled.
  test.skip(!shell, "lmstudio plugin is not installed in this dev instance");
  expect(shell?.kind).toBe("shell");
  expect(shell?.supports_chat_ui).toBe(true);
}

async function openLmStudioTab(
  api: ApiClient,
  tabCleanup: TabCleanup,
  page: Page,
  systemPrompt: string,
): Promise<OpenShellTab> {
  await expectLmStudioShellProvider(api);
  const tab = await tabCleanup.createTab<OpenShellTab>({
    title: "pw-lmstudio-open-shell",
    chat_backend: PLUGIN_ID,
    system_prompt: systemPrompt,
    config: { model: "" },
  });
  expect(tab.pty_id, "open shell must not allocate a PTY").toBeNull();
  await api.focusTab(tab.id);
  await page.locator('[data-testid="tab-item"][data-tab-id="' + tab.tab_id + '"]').click();
  const shell = page.locator('[data-tab-id="' + tab.id + '"] [data-testid="tab-session-chat"]');
  await expect(shell).toBeVisible({ timeout: 30_000 });
  await expect(shell.locator('[data-testid="chat-composer"]')).toBeVisible();
  return tab;
}

async function pushMessages(page: Page, tabId: string, messages: TestChatMessage[]): Promise<void> {
  await page.evaluate(
    ({ targetTabId, targetMessages }) => {
      const testChat = window.__codetermTestChat;
      if (!testChat) throw new Error("window.__codetermTestChat is not installed");
      testChat.pushMessages(targetTabId, targetMessages);
    },
    { targetTabId: tabId, targetMessages: messages },
  );
}

function tabChat(page: Page, tabId: string) {
  return page.locator('[data-tab-id="' + tabId + '"] [data-testid="tab-session-chat"]');
}

function devCli(args: string[]): string {
  const bin = path.join(devDataDir(), "bin", process.platform === "win32" ? "codeterm.exe" : "codeterm");
  return execFileSync(bin, args, {
    cwd: REPO_ROOT,
    encoding: "utf8",
    env: {
      ...process.env,
      CODETERM_API_PORT: "7686",
      CODETERM_API_URL: API_BASE,
      CODETERM_API_TOKEN: readDevToken(),
      CODETERM_DATA_DIR: devDataDir(),
    },
  });
}

test.beforeEach(async ({ api }) => {
  // Skip before try/finally cleanup calls plugin config commands.
  // Missing optional plugins should not fail smoke.
  const providers = await api.get<Array<{ id: string; kind?: string; supports_chat_ui?: boolean }>>(
    "/providers",
  );
  const shell = providers.find((provider) => provider.id === PLUGIN_ID);
  test.skip(!shell, "lmstudio plugin is not installed in this dev instance");
});

test("@smoke lmstudio open shell spawns through the web mirror", async ({
  api,
  webBrowser,
  tabCleanup,
}) => {
  const page = await newWebPage(webBrowser);
  try {
    const tab = await openLmStudioTab(api, tabCleanup, page, "You are a terse local shell.");
    const chat = tabChat(page, tab.id);
    await expect(chat.locator('[data-testid="chat-composer"]')).toBeVisible();

    const tabState = (await api.tabs()).find((candidate) => candidate.id === tab.id);
    expect(tabState?.pty_id).toBeNull();
    expect(tabState?.agent_type ?? null).toBeNull();
  } finally {
    await page.context().close();
  }
});

test("@smoke lmstudio system prompt marker renders as a collapsible card", async ({
  api,
  webBrowser,
  tabCleanup,
}) => {
  const page = await newWebPage(webBrowser);
  try {
    const systemPrompt = "Prefer concise replies.\nNever contact a live model during this test.";
    const tab = await openLmStudioTab(api, tabCleanup, page, systemPrompt);
    const chat = tabChat(page, tab.id);
    await pushMessages(page, tab.id, [
      {
        id: "pw-lmstudio-system-prompt",
        type: "user",
        content: SYSTEM_PROMPT_PREFIX + systemPrompt,
        timestamp: "1700000000000",
        provider: "lmstudio",
      },
    ]);

    const card = chat.locator('[data-testid="chat-system-prompt-card"]');
    await expect(card).toBeVisible();
    await expect(card).toContainText("System prompt");
    await expect(card).not.toContainText("Prefer concise replies");
    await card.getByRole("button", { name: /System prompt/ }).click();
    await expect(card).toContainText("Prefer concise replies");
    await card.getByRole("button", { name: /System prompt/ }).click();
    await expect(card).not.toContainText("Prefer concise replies");
  } finally {
    await page.context().close();
  }
});

test("@smoke lmstudio mocked streaming assistant reply updates in place", async ({
  api,
  webBrowser,
  tabCleanup,
}) => {
  const page = await newWebPage(webBrowser);
  try {
    const tab = await openLmStudioTab(api, tabCleanup, page, "Stream in place.");
    const chat = tabChat(page, tab.id);
    const replyId = "pw-lmstudio-stream-reply";

    await pushMessages(page, tab.id, [
      {
        id: replyId,
        type: "assistant",
        content: "partial reply",
        timestamp: "1700000000001",
        provider: "lmstudio",
      },
    ]);
    const reply = chat
      .locator('[data-testid="chat-message"][data-message-role="assistant"]')
      .filter({ hasText: "partial reply" });
    await expect(reply).toHaveCount(1);

    await pushMessages(page, tab.id, [
      {
        id: replyId,
        type: "assistant",
        content: "partial reply completed",
        timestamp: "1700000000001",
        provider: "lmstudio",
      },
    ]);
    await expect(
      chat
        .locator('[data-testid="chat-message"][data-message-role="assistant"]')
        .filter({ hasText: "partial reply completed" }),
    ).toHaveCount(1);
    await expect(
      chat
        .locator('[data-testid="chat-message"][data-message-role="assistant"]')
        .filter({ hasText: "partial reply" }),
    ).toHaveCount(1);
  } finally {
    await page.context().close();
  }
});

test("@smoke lmstudio mocked tool result renders", async ({ api, webBrowser, tabCleanup }) => {
  const page = await newWebPage(webBrowser);
  try {
    const tab = await openLmStudioTab(api, tabCleanup, page, "Render tool results.");
    const chat = tabChat(page, tab.id);
    await pushMessages(page, tab.id, [
      {
        id: "pw-lmstudio-tool-result",
        type: "tool_result",
        content: "tool_result: local search returned 3 rows",
        timestamp: "1700000000002",
        provider: "lmstudio",
        toolId: "local-search",
      },
    ]);

    const result = chat
      .locator('[data-testid="chat-message"][data-message-role="tool_result"]')
      .filter({ hasText: "local search returned 3 rows" });
    await expect(result).toBeVisible();
    await expect(result).toContainText("local search returned 3 rows");
  } finally {
    await page.context().close();
  }
});

test("@smoke lmstudio config edit preserves open shell transcript", async ({
  api,
  webBrowser,
  tabCleanup,
}) => {
  test.setTimeout(120_000);
  const page = await newWebPage(webBrowser);
  try {
    const tab = await openLmStudioTab(api, tabCleanup, page, "Survive config reload.");
    const chat = tabChat(page, tab.id);
    const beforeMarker = "CONFIG-PRESERVED-" + Date.now();
    const afterMarker = "CONFIG-STILL-WORKS-" + Date.now();

    await pushMessages(page, tab.id, [
      {
        id: "pw-lmstudio-before-config",
        type: "assistant",
        content: beforeMarker,
        timestamp: "1700000000003",
        provider: "lmstudio",
      },
    ]);
    await expect(chat).toContainText(beforeMarker);

    devCli([
      "plugin",
      "config",
      PLUGIN_ID,
      "--set",
      "model=pw-preserve-" + Date.now(),
    ]);

    await expect(chat).toContainText(beforeMarker);
    await pushMessages(page, tab.id, [
      {
        id: "pw-lmstudio-after-config",
        type: "assistant",
        content: afterMarker,
        timestamp: "1700000000004",
        provider: "lmstudio",
      },
    ]);
    await expect(chat).toContainText(afterMarker);
    await expect(chat.locator('[data-testid="chat-composer"]')).toBeVisible();
  } finally {
    devCli(["plugin", "config", PLUGIN_ID, "--set", "model="]);
    await page.context().close();
  }
});
