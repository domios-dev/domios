// Run: npx tsx src/lib/orchestratorOnboard.test.ts

import { canMakeOrchestrator, isAgentCapableTab } from "./orchestratorOnboardEligibility";
import type { Tab } from "./types";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function mkTab(overrides: Partial<Tab> = {}): Tab {
  return {
    id: "tab-1",
    pty_id: null,
    tab_type: "Terminal",
    title: "Test",
    cwd: null,
    ...overrides,
  };
}

test("ineligible: plain shell Tab", () => {
  assert(!isAgentCapableTab(mkTab()), "plain shell is not agent-capable");
  assert(!canMakeOrchestrator(mkTab()), "plain shell cannot be made orchestrator");
});

test("eligible: PTY agent Tab", () => {
  const tab = mkTab({ agent_type: "claude" });
  assert(isAgentCapableTab(tab), "agent_type marks agent-capable");
  assert(canMakeOrchestrator(tab), "agent Tab is eligible");
});

test("eligible: chatBackend Tab", () => {
  const tab = mkTab({
    backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "s1" },
  });
  assert(isAgentCapableTab(tab), "chat_backend marks agent-capable");
  assert(canMakeOrchestrator(tab), "chatBackend Tab is eligible");
});

test("ineligible: already orchestrator", () => {
  assert(
    !canMakeOrchestrator(mkTab({ agent_type: "claude", role: "Orchestrator", can_spawn: true })),
    "orchestrator blocked",
  );
});

test("eligible: worker can atomically transition out of hierarchy", () => {
  assert(
    canMakeOrchestrator(
      mkTab({ agent_type: "claude", role: "Worker", parent_tab_id: "orch-1" }),
    ),
    "nested worker can become an orchestrator",
  );
});

test("ineligible: protected Tab", () => {
  assert(
    !canMakeOrchestrator(mkTab({ agent_type: "claude", protected: true })),
    "protected blocked",
  );
});

test("ineligible: null Tab", () => {
  assert(!canMakeOrchestrator(null), "null blocked");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`ok ${name}`);
  } catch (e) {
    failed += 1;
    console.error(`FAIL ${name}:`, e);
  }
}
if (failed > 0) process.exit(1);
