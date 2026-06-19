import { chromium, test as base, type Browser, type Page } from "@playwright/test";
import { ApiClient, readDevToken } from "./api-client";
import { apiReset } from "./reset";
import { API_BASE, connectUi, isHealthy, type UiSession } from "./dev-console";

type WorkerFixtures = {
  uiSession: ManagedUiSession;
  webBrowser: Browser;
};

export type ManagedUiSession = UiSession & {
  reconnect: () => Promise<ManagedUiSession>;
};

export type PaneCleanup = {
  track: (paneId: string | undefined | null) => void;
  createPane: <T extends { id?: string } = { id: string }>(body?: Record<string, unknown>) => Promise<T>;
  closeUnprotectedPanes: () => Promise<void>;
};

export type SpawnAgent = <T extends { pane_id: string } = { pane_id: string }>(
  body: Record<string, unknown>,
) => Promise<T>;

export type PollMarkerInDomOptions = {
  page?: Page;
  paneId?: string;
  timeout?: number;
  polling?: number;
  normalize?: boolean;
  includeChatMessages?: boolean;
};

export type MediaMocks = {
  mockUpload: (page: Page, response?: { path?: string; size?: number }) => Promise<void>;
  mockTranscription: (
    page: Page,
    response: { line?: string; status?: number; body?: string; delayMs?: number },
  ) => Promise<void>;
  installVoiceRecorder: (page: Page, script: string) => Promise<void>;
};

type TestFixtures = {
  api: ApiClient;
  ui: ManagedUiSession;
  paneCleanup: PaneCleanup;
  spawnAgent: SpawnAgent;
  pollMarkerInDom: (marker: string, options?: PollMarkerInDomOptions) => Promise<void>;
  mediaMocks: MediaMocks;
};

export const test = base.extend<TestFixtures, WorkerFixtures>({
  uiSession: [
    async ({}, use) => {
      let current = await connectUi();
      const session: ManagedUiSession = {
        get browser() {
          return current.browser;
        },
        get page() {
          return current.page;
        },
        reconnect: async () => {
          await current.browser.close().catch(() => {});
          current = await connectUi();
          return session;
        },
      };
      try {
        await use(session);
      } finally {
        await current.browser.close();
      }
    },
    { scope: "worker" },
  ],

  webBrowser: [
    async ({}, use) => {
      const browser = await chromium.launch(
        process.platform === "win32" ? { channel: "msedge" } : {},
      );
      try {
        await use(browser);
      } finally {
        await browser.close();
      }
    },
    { scope: "worker" },
  ],

  ui: async ({ uiSession }, use) => {
    if (!(await isHealthy())) {
      throw new Error("dev console health check failed before Playwright test");
    }
    await use(uiSession);
  },

  api: async ({}, use) => {
    const client = new ApiClient();
    try {
      await use(client);
    } finally {
      await apiReset(client);
    }
  },

  paneCleanup: async ({ api }, use) => {
    const paneIds = new Set<string>();
    const cleanup: PaneCleanup = {
      track: (paneId) => {
        if (paneId) paneIds.add(paneId);
      },
      createPane: async <T extends { id?: string } = { id: string }>(body?: Record<string, unknown>) => {
        const pane = await api.createPane(body) as T;
        cleanup.track(pane.id);
        return pane;
      },
      closeUnprotectedPanes: async () => {
        for (const pane of await api.panes()) {
          if (!pane.protected) await api.closePane(pane.id).catch(() => {});
        }
      },
    };

    try {
      await use(cleanup);
    } finally {
      for (const paneId of Array.from(paneIds).reverse()) {
        await api.closePane(paneId).catch(() => {});
      }
    }
  },

  spawnAgent: async ({ api, paneCleanup }, use) => {
    await use(async <T extends { pane_id: string } = { pane_id: string }>(body: Record<string, unknown>) => {
      const spawned = await api.post<T>("/agents/spawn", body);
      paneCleanup.track(spawned.pane_id);
      return spawned;
    });
  },

  pollMarkerInDom: async ({ ui }, use) => {
    await use(async (marker, options = {}) => {
      const page = options.page ?? ui.page;
      await page.waitForFunction(
        ({ marker: expected, paneId, normalize, includeChatMessages }) => {
          const clean = (text: string) =>
            normalize ? text.replace(/[^A-Za-z0-9]/g, "").toLowerCase() : text;
          const needle = clean(expected);
          const pane = paneId ? document.querySelector(`[data-pane-id="${paneId}"]`) : null;
          const rootSelectors = ".xterm-accessibility, .xterm-accessibility-tree, .xterm-rows";
          const roots = pane
            ? Array.from(pane.querySelectorAll(rootSelectors))
            : Array.from(document.querySelectorAll(rootSelectors));
          const chatScope = pane ?? document;
          const chatMessages = includeChatMessages
            ? Array.from(chatScope.querySelectorAll('[data-testid="chat-message"]'))
            : [];
          return roots
            .concat(chatMessages)
            .filter((el) => el.tagName !== "STYLE" && el.tagName !== "SCRIPT")
            .some((el) => clean((el as HTMLElement).innerText || el.textContent || "").includes(needle));
        },
        {
          marker,
          paneId: options.paneId,
          normalize: options.normalize ?? false,
          includeChatMessages: options.includeChatMessages ?? false,
        },
        { timeout: options.timeout ?? 240_000, polling: options.polling ?? 1_000 },
      );
    });
  },

  mediaMocks: async ({}, use) => {
    await use({
      mockUpload: async (page, response = {}) => {
        await page.route("**/api/upload", (route) =>
          route.fulfill({
            contentType: "application/json",
            body: JSON.stringify({
              path: response.path ?? "/tmp/pw-voice.ogg",
              size: response.size ?? 8192,
            }),
          }),
        );
      },
      mockTranscription: async (page, response) => {
        await page.route("**/api/transcribe-voice", async (route) => {
          if (response.delayMs) {
            await new Promise((resolve) => setTimeout(resolve, response.delayMs));
          }
          await route.fulfill({
            status: response.status,
            contentType: "application/json",
            body: response.body ?? JSON.stringify({ line: response.line ?? "" }),
          });
        });
      },
      installVoiceRecorder: async (page, script) => {
        await page.addInitScript(script);
      },
    });
  },
});

export { expect } from "@playwright/test";
export type { Browser, Page };
export { API_BASE, readDevToken };
