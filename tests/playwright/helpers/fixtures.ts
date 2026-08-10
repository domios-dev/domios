import { chromium, test as base, type Browser, type Page } from "@playwright/test";
import { ApiClient, readDevToken } from "./api-client";
import { currentDevInstance, type DevInstance } from "./dev-instance";
import { apiReset, diffReset, snapshotBaseline } from "./reset";
import {
  API_BASE,
  connectUi,
  ensureDevConsole,
  healthMismatchMessage,
  isHealthy,
  probeHealth,
  type UiSession,
} from "./dev-console";
import { assertServedBundleIsFresh } from "./dist-freshness";

type WorkerFixtures = {
  devInstance: DevInstance;
  uiSession: ManagedUiSession;
  webBrowser: Browser;
  resetTracker: { lastFile: string };
};

export type ManagedUiSession = UiSession & {
  reconnect: () => Promise<ManagedUiSession>;
};

export type TabCleanup = {
  track: (tabId: string | undefined | null) => void;
  createTab: <T extends { id?: string } = { id: string }>(body?: Record<string, unknown>) => Promise<T>;
  closeUnprotectedTabs: () => Promise<void>;
};

export type SpawnAgent = <T extends { tab_id: string } = { tab_id: string }>(
  body: Record<string, unknown>,
) => Promise<T>;

export type PollMarkerInDomOptions = {
  page?: Page;
  tabId?: string;
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
  tabCleanup: TabCleanup;
  spawnAgent: SpawnAgent;
  pollMarkerInDom: (marker: string, options?: PollMarkerInDomOptions) => Promise<void>;
  mediaMocks: MediaMocks;
};

export const DEV_FIXTURE_TIMEOUT_MS = 660_000;

type MediaUploadMock = { path: string; size: number };
type MediaTranscriptionMock = { line?: string; status?: number; body?: string; delayMs?: number };

async function installMediaFetchMocks(page: Page): Promise<void> {
  const script = () => {
    const win = window as typeof window & {
      __ctMediaFetchMocksInstalled?: boolean;
      __ctMediaUploadMock?: MediaUploadMock;
      __ctMediaTranscriptionMock?: MediaTranscriptionMock;
    };
    if (win.__ctMediaFetchMocksInstalled) return;
    win.__ctMediaFetchMocksInstalled = true;
    const originalFetch = win.fetch.bind(win);
    const requestUrl = (input: RequestInfo | URL) =>
      new URL(input instanceof Request ? input.url : String(input), win.location.href);
    win.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = requestUrl(input);
      if (url.pathname === "/api/upload" && win.__ctMediaUploadMock) {
        return new Response(JSON.stringify(win.__ctMediaUploadMock), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.pathname === "/api/transcribe-voice" && win.__ctMediaTranscriptionMock) {
        const mock = win.__ctMediaTranscriptionMock;
        // PW_JUSTIFIED_SLEEP: stimulus — the mocked transcription latency IS the test input.
        if (mock.delayMs) await new Promise((resolve) => setTimeout(resolve, mock.delayMs));
        return new Response(mock.body ?? JSON.stringify({ line: mock.line ?? "" }), {
          status: mock.status ?? 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return originalFetch(input, init);
    };
  };
  await page.addInitScript(script);
  await page.evaluate(script).catch(() => {});
}

export const test = base.extend<TestFixtures, WorkerFixtures>({
  devInstance: [
    async ({}, use, workerInfo) => {
      const instance = currentDevInstance();
      if (instance.slot !== workerInfo.parallelIndex) {
        throw new Error(
          `worker ${workerInfo.workerIndex} resolved ${instance.name} for slot ${instance.slot}, expected parallel slot ${workerInfo.parallelIndex}`,
        );
      }
      console.log(
        `[playwright-instance] worker=${workerInfo.workerIndex} slot=${workerInfo.parallelIndex} instance=${instance.name || "default"} api=${instance.apiPort}`,
      );
      await use(instance);
    },
    { scope: "worker", auto: true },
  ],

  uiSession: [
    async ({ devInstance }, use) => {
      let current = await connectUi(devInstance);
      const session: ManagedUiSession = {
        get browser() {
          return current.browser;
        },
        get page() {
          return current.page;
        },
        reconnect: async () => {
          await current.browser.close().catch(() => {});
          current = await connectUi(devInstance);
          return session;
        },
      };
      try {
        await use(session);
      } finally {
        await current.browser.close();
      }
    },
    { scope: "worker", timeout: DEV_FIXTURE_TIMEOUT_MS },
  ],

  webBrowser: [
    async ({ devInstance }, use) => {
      await assertServedBundleIsFresh(devInstance);
      const browser = await chromium.launch(
        process.platform === "win32" ? { channel: "msedge" } : {},
      );
      try {
        await use(browser);
      } finally {
        await browser.close();
      }
    },
    { scope: "worker", timeout: DEV_FIXTURE_TIMEOUT_MS },
  ],

  ui: async ({ devInstance, uiSession }, use) => {
    const verdict = await probeHealth(devInstance);
    if (!verdict.ok) throw new Error(healthMismatchMessage(devInstance, verdict));
    await use(uiSession);
  },

  resetTracker: [
    async ({}, use) => {
      await use({ lastFile: "" });
    },
    { scope: "worker" },
  ],

  // Diff-based teardown closes only what the test created; the blanket sweep
  // runs once per spec file as a stray-state safety net. A daemon that died
  // mid-previous-test is revived here so one crash fails one test, not a cascade
  // (own timeout: revival can rebuild dev, which never fits a per-test budget).
  api: [
    async ({ devInstance, resetTracker }, use, testInfo) => {
      if (!(await isHealthy(devInstance))) {
        await ensureDevConsole(devInstance);
      }
      const client = new ApiClient(undefined, devInstance);
      if (resetTracker.lastFile !== testInfo.file) {
        resetTracker.lastFile = testInfo.file;
        await apiReset(client);
      }
      const baseline = await snapshotBaseline(client);
      try {
        await use(client);
      } finally {
        await diffReset(client, baseline).catch(() => {});
      }
    },
    { timeout: DEV_FIXTURE_TIMEOUT_MS },
  ],

  tabCleanup: async ({ api }, use) => {
    const tabIds = new Set<string>();
    const cleanup: TabCleanup = {
      track: (tabId) => {
        if (tabId) tabIds.add(tabId);
      },
      createTab: async <T extends { id?: string } = { id: string }>(body?: Record<string, unknown>) => {
        const tab = await api.createTab(body) as T;
        cleanup.track(tab.id);
        return tab;
      },
      closeUnprotectedTabs: async () => {
        for (const tab of await api.tabs()) {
          if (!tab.protected) await api.closeTab(tab.id).catch(() => {});
        }
      },
    };

    try {
      await use(cleanup);
    } finally {
      for (const tabId of Array.from(tabIds).reverse()) {
        await api.closeTab(tabId).catch(() => {});
      }
    }
  },

  spawnAgent: async ({ api, tabCleanup }, use) => {
    await use(async <T extends { tab_id: string } = { tab_id: string }>(body: Record<string, unknown>) => {
      const spawned = await api.post<T>("/agents/spawn", body);
      tabCleanup.track(spawned.tab_id);
      return spawned;
    });
  },

  pollMarkerInDom: async ({ ui }, use) => {
    await use(async (marker, options = {}) => {
      const page = options.page ?? ui.page;
      await page.waitForFunction(
        ({ marker: expected, tabId, normalize, includeChatMessages }) => {
          const clean = (text: string) =>
            normalize ? text.replace(/[^A-Za-z0-9]/g, "").toLowerCase() : text;
          const needle = clean(expected);
          const tab = tabId ? document.querySelector(`[data-tab-id="${tabId}"]`) : null;
          const rootSelectors = ".xterm-accessibility, .xterm-accessibility-tree, .xterm-rows";
          const roots = tab
            ? Array.from(tab.querySelectorAll(rootSelectors))
            : Array.from(document.querySelectorAll(rootSelectors));
          const chatScope = tab ?? document;
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
          tabId: options.tabId,
          normalize: options.normalize ?? false,
          includeChatMessages: options.includeChatMessages ?? false,
        },
        { timeout: options.timeout ?? 240_000, polling: options.polling ?? 250 },
      );
    });
  },

  mediaMocks: async ({}, use) => {
    await use({
      mockUpload: async (page, response = {}) => {
        await installMediaFetchMocks(page);
        const mock = {
          path: response.path ?? "/tmp/pw-voice.ogg",
          size: response.size ?? 8192,
        };
        await page.addInitScript((nextMock) => {
          (window as typeof window & { __ctMediaUploadMock?: MediaUploadMock }).__ctMediaUploadMock = nextMock;
        }, mock);
        await page.evaluate((nextMock) => {
          (window as typeof window & { __ctMediaUploadMock?: MediaUploadMock }).__ctMediaUploadMock = nextMock;
        }, mock);
      },
      mockTranscription: async (page, response) => {
        await installMediaFetchMocks(page);
        await page.addInitScript((mock) => {
          (window as typeof window & { __ctMediaTranscriptionMock?: MediaTranscriptionMock }).__ctMediaTranscriptionMock = mock;
        }, response);
        await page.evaluate((mock) => {
          (window as typeof window & { __ctMediaTranscriptionMock?: MediaTranscriptionMock }).__ctMediaTranscriptionMock = mock;
        }, response);
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
