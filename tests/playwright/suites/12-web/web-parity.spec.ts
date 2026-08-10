/**
 * WHAT: Browser-mode web UI matches desktop surfaces for core navigation, chrome, and actions.
 * WHY: The mirror shares React logic with desktop but runs through a different browser entrypoint and auth path.
 * ORACLE: Stable data-testid surfaces, settings state, titlebar controls, and workspace state agree with backend snapshots.
 */
import {
  expect,
  test,
  type Browser,
  type Page,
  API_BASE,
  readDevToken,
} from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";

const WEB_URL = `${API_BASE}/`;
const RAW_PROVIDER_IDS = new Set([
  "claude",
  "codex",
  "cursor",
  "opencode",
  "aider",
  "goose",
]);

type ProviderDescriptor = {
  id: string;
  display_name: string;
  transport?: string;
};

type ProviderMenuConfig = {
  agents?: Record<string, { visible_in_menu?: boolean }>;
};

type MuxState = {
  active_workspace_id?: string | null;
  workspaces: {
    id: string;
    name: string;
    tabs: { id: string; tab_id?: string | null }[];
  }[];
  tabs: Record<
    string,
    { id: string; title?: string | null; agent_type?: string | null }
  >;
};

type SpawnResult = {
  tab_id: string;
  tab_id?: string;
  title?: string;
};

type KeepAwakeStatus = {
  active: boolean;
  display: boolean;
  expires_at: number | null;
};

async function newWebPage(
  browser: Browser,
  viewport?: { width: number; height: number },
): Promise<Page> {
  const ctx = await browser.newContext({
    viewport: viewport ?? { width: 1280, height: 800 },
    serviceWorkers: "block",
  });
  const page = await ctx.newPage();
  await ctx.route("**/api/mesh/peers", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        paired: [],
        discovered: [],
        incoming_requests: [],
      }),
    }),
  );
  return page;
}

async function openWeb(page: Page): Promise<void> {
  await page.goto(`${WEB_URL}#token=${encodeURIComponent(readDevToken())}`, {
    waitUntil: "domcontentloaded",
  });
  await page
    .locator(
      '[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]',
    )
    .first()
    .waitFor({ state: "visible", timeout: 30_000 });
}

async function openSidebarIfNeeded(page: Page): Promise<void> {
  const toggle = page.locator('[data-testid="web-sidebar-open"]');
  if (await toggle.isVisible().catch(() => false)) await toggle.click();
}

async function expandWorkspaceSidebar(page: Page): Promise<void> {
  const workspace = page.locator('[data-testid="workspace-item"]').first();
  if (!(await workspace.isVisible().catch(() => false))) return;
  const expand = workspace.locator('button[title="Expand"]');
  if (await expand.isVisible().catch(() => false)) await expand.click();
}

function titleTokens(provider: ProviderDescriptor): string[] {
  return [provider.id, provider.display_name, ...RAW_PROVIDER_IDS]
    .map((s) => s.trim().toLowerCase())
    .filter(Boolean);
}

function assertGeneratedSessionTitle(
  title: string,
  provider: ProviderDescriptor,
  seen: Set<string>,
): void {
  const normalized = title.trim();
  expect(normalized, `generated title for ${provider.id}`).toMatch(
    /^[A-Z][a-z]+(?:\d+)?$/,
  );
  expect(
    normalized,
    `generated title should be unique for ${provider.id}`,
  ).not.toHaveLength(0);
  expect(seen.has(normalized), `duplicate generated title: ${normalized}`).toBe(
    false,
  );
  for (const raw of titleTokens(provider)) {
    expect(
      normalized.toLowerCase(),
      `title must not be raw provider/display name ${raw}`,
    ).not.toBe(raw);
  }
  expect(
    normalized.toLowerCase(),
    "title must not be an agent command",
  ).not.toContain(" ");
  seen.add(normalized);
}

function ptyProviders(providers: ProviderDescriptor[]): ProviderDescriptor[] {
  return providers.filter((provider) => provider.transport !== "chat_backend");
}

function menuVisibleProviders(
  providers: ProviderDescriptor[],
  config: ProviderMenuConfig,
): ProviderDescriptor[] {
  return providers.filter(
    (provider) => config.agents?.[provider.id]?.visible_in_menu !== false,
  );
}

async function expectProviderMenuContract(
  page: Page,
  providers: ProviderDescriptor[],
  config: ProviderMenuConfig,
): Promise<void> {
  await openSidebarIfNeeded(page);
  const workspace = page.locator('[data-testid="workspace-item"]').first();
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  await expandWorkspaceSidebar(page);
  await page
    .getByRole("button", { name: /^New session$/ })
    .first()
    .click();

  for (const provider of providers) {
    const item = page.getByRole("button", {
      name: `New ${provider.display_name} Session`,
      exact: true,
    });
    if (config.agents?.[provider.id]?.visible_in_menu === false) {
      await expect(
        item,
        `${provider.id} is explicitly hidden by agents config`,
      ).toHaveCount(0);
    } else {
      await expect(
        item,
        `${provider.id} is a menu-visible PTY provider`,
      ).toBeVisible();
    }
  }
  await page.keyboard.press("Escape");
}

async function currentMux(api: {
  get<T>(path: string): Promise<T>;
}): Promise<MuxState> {
  return api.get<MuxState>("/mux/state");
}

function allTabIds(mux: MuxState): Set<string> {
  return new Set(Object.keys(mux.tabs));
}

function activeWorkspaceId(mux: MuxState): string {
  const active =
    mux.active_workspace_id ??
    mux.workspaces.find((w) => w.tabs.some((t) => t.tab_id))?.id;
  if (!active) throw new Error("no active workspace found for web parity test");
  return active;
}

function tabTabId(mux: MuxState, tabId: string): string | null {
  for (const workspace of mux.workspaces) {
    for (const tab of workspace.tabs) {
      if (tab.tab_id === tabId) return tab.id;
    }
  }
  return null;
}

async function waitForSpawnedTab(
  api: { get<T>(path: string): Promise<T> },
  beforeTabIds: Set<string>,
  providerId: string,
): Promise<{ tabId: string; tabId: string; title: string }> {
  return pollUntil(
    async () => {
      const mux = await currentMux(api);
      for (const [tabId, tab] of Object.entries(mux.tabs)) {
        if (beforeTabIds.has(tabId) || tab.agent_type !== providerId)
          continue;
        const tabId = tabTabId(mux, tabId);
        const title = tab.title?.trim();
        if (tabId && title) return { tabId, tabId, title };
      }
      return undefined;
    },
    30_000,
    500,
    `spawned ${providerId} tab`,
  );
}

async function clickNewSession(
  page: Page,
  provider: ProviderDescriptor,
): Promise<void> {
  await openSidebarIfNeeded(page);
  const workspace = page.locator('[data-testid="workspace-item"]').first();
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  await expandWorkspaceSidebar(page);
  const newSession = page
    .getByRole("button", { name: /^New session$/ })
    .first();
  await expect(newSession).toBeVisible({ timeout: 10_000 });
  await newSession.click();
  const item = page.getByRole("button", {
    name: new RegExp(`^New ${escapeRegExp(provider.display_name)} Session$`),
  });
  await expect(item).toBeVisible({ timeout: 10_000 });
  await item.click();
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function expectSettingsDeepLink(
  page: Page,
  section: string,
): Promise<void> {
  await page.locator('[data-testid="titlebar-settings"]').click();
  await expect(page.locator('[data-testid="settings-menu"]')).toBeVisible({
    timeout: 10_000,
  });
  await page.locator(`[data-testid="settings-menu-${section}"]`).click();
  await expect(page.locator('[data-testid="settings-panel"]')).toBeVisible({
    timeout: 10_000,
  });
  await expect(
    page.locator('[data-testid="settings-header-title"]'),
  ).toContainText(section === "general" ? "General" : "Agents");
  await expect(page.locator('[data-testid="settings-menu"]')).not.toBeVisible({
    timeout: 10_000,
  });
  await page.locator('[data-testid="settings-close"]').click();
  await expect(page.locator('[data-testid="settings-panel"]')).not.toBeVisible({
    timeout: 10_000,
  });
}

async function expectKeepAwakeRoundTrip(
  page: Page,
  api: {
    get<T>(path: string): Promise<T>;
    post<T>(path: string, body?: unknown): Promise<T>;
  },
): Promise<void> {
  await api.post("/keepawake/stop").catch(() => undefined);
  await pollUntil(
    async () => {
      const status = await api.get<KeepAwakeStatus>("/keepawake");
      return status.active ? undefined : status;
    },
    10_000,
    250,
    "keepawake stopped before web round-trip",
  );

  await page.locator('[data-testid="keepawake-tray"]').click();
  await page.getByTitle("1 hour").click();
  await pollUntil(
    async () => {
      const status = await api.get<KeepAwakeStatus>("/keepawake");
      return status.active && status.expires_at !== null ? status : undefined;
    },
    10_000,
    250,
    "keepawake starts from web titlebar",
  );

  await page.locator('[data-testid="keepawake-tray"]').click();
  await page.getByRole("button", { name: "End" }).click();
  await pollUntil(
    async () => {
      const status = await api.get<KeepAwakeStatus>("/keepawake");
      return status.active ? undefined : status;
    },
    10_000,
    250,
    "keepawake stops from web titlebar",
  );
}

async function expectTitlebarLayout(
  page: Page,
  viewport: { width: number; height: number },
): Promise<void> {
  const titlebar = page.locator('[data-testid="titlebar"]');
  await expect(titlebar).toBeVisible({ timeout: 20_000 });
  await expect(
    titlebar.locator('[data-testid="titlebar-extensions"]'),
  ).toBeVisible();
  await expect(
    titlebar.locator('[data-testid="keepawake-tray"]'),
  ).toBeVisible();
  await expect(
    titlebar.locator('[data-testid="titlebar-settings"]'),
  ).toBeVisible();
  // The host switcher left the titlebar for the sidebar GENERAL header
  // (Sidebar.tsx renders <WebPeerPicker compact/> there). web.spec's chrome
  // golden owns its new home; this helper holds the titlebar to what it kept.
  await expect(titlebar.locator('[data-testid="peer-picker"]')).toHaveCount(0);

  const metrics = await page.evaluate(() => {
    const rectOf = (selector: string) => {
      const el = document.querySelector(selector);
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
        width: r.width,
        height: r.height,
      };
    };
    return {
      titlebar: rectOf('[data-testid="titlebar"]'),
      burger: rectOf('[data-testid="web-sidebar-open"]'),
      scrollWidth: document.documentElement.scrollWidth,
      viewportWidth: window.innerWidth,
    };
  });

  expect(metrics.titlebar, "titlebar box").not.toBeNull();
  expect(
    metrics.scrollWidth,
    "web titlebar should not create horizontal scroll",
  ).toBeLessThanOrEqual(viewport.width);
  expect(
    metrics.titlebar!.top,
    "titlebar respects top safe-area inset",
  ).toBeGreaterThanOrEqual(0);
  expect(metrics.titlebar!.right, "titlebar fits viewport").toBeLessThanOrEqual(
    metrics.viewportWidth + 1,
  );
  if (metrics.burger) {
    // The hamburger is a child of the titlebar row (Titlebar.tsx), not a
    // floating control below it.
    expect(
      metrics.burger.top,
      "sidebar toggle sits inside the titlebar row",
    ).toBeGreaterThanOrEqual(metrics.titlebar!.top - 1);
    expect(
      metrics.burger.bottom,
      "sidebar toggle stays inside the titlebar row",
    ).toBeLessThanOrEqual(metrics.titlebar!.bottom + 1);
  }
}

test.describe("@golden web-native parity", () => {
  test.describe.configure({ mode: "serial" });

  test("@golden T1 naming gate: web + New session and REST no-title spawns keep generated names", async ({
    api,
    webBrowser,
    tabCleanup,
  }) => {
    test.setTimeout(300_000);
    const pty = ptyProviders((await api.providers()) as ProviderDescriptor[]);
    const providerConfig = await api.get<ProviderMenuConfig>("/config");
    const providers = menuVisibleProviders(pty, providerConfig);
    expect(
      providers.length,
      "at least one PTY provider is required for the naming gate",
    ).toBeGreaterThan(0);

    const page = await newWebPage(webBrowser);
    const seenTitles = new Set<string>();
    try {
      await openWeb(page);
      await expectProviderMenuContract(page, pty, providerConfig);
      const workspaceId = activeWorkspaceId(await currentMux(api));

      for (const provider of providers) {
        const before = allTabIds(await currentMux(api));
        await clickNewSession(page, provider);
        const spawned = await waitForSpawnedTab(api, before, provider.id);
        tabCleanup.track(spawned.tabId);
        await expect(
          page.locator(
            `[data-testid="tab-item"][data-tab-id="${spawned.tabId}"]`,
          ),
        ).toContainText(spawned.title, { timeout: 20_000 });
        assertGeneratedSessionTitle(spawned.title, provider, seenTitles);
      }

      const restProvider = providers[0];
      const restSpawn = await api.post<SpawnResult>("/agents/spawn", {
        agent: restProvider.id,
        workspace_id: workspaceId,
        skip_permissions: true,
        activate: true,
      });
      tabCleanup.track(restSpawn.tab_id);
      const restTab = await pollUntil(
        async () => {
          const mux = await currentMux(api);
          const tab = mux.tabs[restSpawn.tab_id];
          const tabId = tabTabId(mux, restSpawn.tab_id);
          return tab?.title && tabId
            ? { title: tab.title, tabId }
            : undefined;
        },
        30_000,
        500,
        "REST no-title spawn title",
      );
      await expect(
        page.locator(
          `[data-testid="tab-item"][data-tab-id="${restTab.tabId}"]`,
        ),
      ).toContainText(restTab.title, { timeout: 20_000 });
      assertGeneratedSessionTitle(restTab.title, restProvider, seenTitles);
    } finally {
      await page.context().close();
    }
  });

  test("@golden T3 titlebar strip gate: desktop settings deep link and keep-awake round-trip", async ({
    api,
    webBrowser,
  }) => {
    test.setTimeout(180_000);
    const page = await newWebPage(webBrowser);
    try {
      await api.post("/notifications/clear").catch(() => undefined);
      await openWeb(page);
      await expectTitlebarLayout(page, { width: 1280, height: 800 });

      // The two strip controls that own real behaviour: a settings deep link
      // that lands on the right section and closes cleanly, and keep-awake
      // driven from the web titlebar with the daemon confirming both edges.
      await expectSettingsDeepLink(page, "general");
      await expectKeepAwakeRoundTrip(page, api);
    } finally {
      await api.post("/keepawake/stop").catch(() => undefined);
      await api.post("/notifications/clear").catch(() => undefined);
      await page.context().close();
    }
  });

  test("@golden T3 titlebar strip gate: mobile safe-area layout stays reachable", async ({
    api,
    webBrowser,
  }) => {
    // `workspace-item` is rendered only for ORDINARY workspaces; a fresh instance
    // holds just the protected General, which lives in general-section. Waiting on
    // whatever the shared instance happens to contain makes this test's result a
    // function of another lane's timing, so it seeds the card it asserts.
    const workspaceName = `pw-parity-mobile-${Date.now()}`;
    await api.createWorkspace(workspaceName);
    const workspaceId = (await api.workspaces()).find((w) => w.name === workspaceName)?.id;
    expect(workspaceId, "safe-area fixture workspace was not created").toBeTruthy();

    const page = await newWebPage(webBrowser, { width: 375, height: 812 });
    try {
      await openWeb(page);
      await expectTitlebarLayout(page, { width: 375, height: 812 });

      const burger = page.locator('[data-testid="web-sidebar-open"]');
      await expect(burger).toBeVisible({ timeout: 10_000 });
      await burger.click();
      await expect(
        page.locator(`[data-testid="workspace-item"][data-workspace-name="${workspaceName}"]`),
        "seeded ordinary workspace did not render as a sidebar card",
      ).toBeVisible({ timeout: 10_000 });
    } finally {
      await page.context().close();
      if (workspaceId) await api.closeWorkspace(workspaceId).catch(() => {});
    }
  });
});
