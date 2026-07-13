import { test as base, DEV_FIXTURE_TIMEOUT_MS } from "./fixtures";
import { ApiClient } from "./api-client";
import { apiReset } from "./reset";
import { preparePerfSeed, stubEnvActive, STUB_ENV_ABSENT_SKIP, type PerfSeed } from "./perf-seed";

type NoTestFixtures = Record<never, never>;

// Seed owns its ApiClient and runs the ONLY reset before seeding; shared api (thus
// per-test reset) is absent, so @perf bodies must stay read-only on seeded topology.
export const test = base.extend<NoTestFixtures, { perfSeed: PerfSeed }>({
  perfSeed: [
    async ({ devInstance }, use) => {
      const api = new ApiClient(undefined, devInstance);
      await apiReset(api);
      const seed = await preparePerfSeed(api);
      try {
        await use(seed);
      } finally {
        await apiReset(api).catch(() => {});
      }
    },
    { scope: "worker", timeout: DEV_FIXTURE_TIMEOUT_MS },
  ],
});

export { expect } from "./fixtures";
export type { Page } from "./fixtures";
export { stubEnvActive, STUB_ENV_ABSENT_SKIP };
