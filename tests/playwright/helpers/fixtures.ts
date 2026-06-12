import { test as base } from "@playwright/test";
import { ApiClient } from "./api-client";
import { apiReset } from "./reset";
import { connectUi, isHealthy, type UiSession } from "./dev-console";

type WorkerFixtures = {
  uiSession: UiSession;
};

type TestFixtures = {
  api: ApiClient;
  ui: UiSession;
};

export const test = base.extend<TestFixtures, WorkerFixtures>({
  uiSession: [
    async ({}, use) => {
      const session = await connectUi();
      try {
        await use(session);
      } finally {
        await session.browser.close();
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
});

export { expect } from "@playwright/test";

