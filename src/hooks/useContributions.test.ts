// Run: npx tsx src/hooks/useContributions.test.ts

import type { ContributionsChangedPayload } from "./useContributions";
import { shouldRefreshAgentProviders } from "./useAgentProviders";

let failed = 0;

function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (e) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(e);
  }
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

test("contributions payload keeps the kind contract", () => {
  const payload: ContributionsChangedPayload = { kind: "agent_provider" };
  assert(payload.kind === "agent_provider", "kind should be agent_provider");
});

test("agent provider refresh accepts the models-updated event fixture", () => {
  const fixture = "models-updated";
  assert(shouldRefreshAgentProviders(fixture), "models-updated should trigger a refresh");
});

console.log(`useContributions: ${failed === 0 ? "all passed" : `${failed} failed`}`);
if (failed > 0) process.exit(1);
