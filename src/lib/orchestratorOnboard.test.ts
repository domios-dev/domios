// Run: npx tsx src/lib/orchestratorOnboard.test.ts

import { canMakeOrchestrator, isAgentCapablePane } from "./orchestratorOnboardEligibility";
import type { Pane } from "./types";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function mkPane(overrides: Partial<Pane> = {}): Pane {
  return {
    id: "pane-1",
    pty_id: null,
    pane_type: "Terminal",
    title: "Test",
    cwd: null,
    ...overrides,
  };
}

test("ineligible: plain shell Tab", () => {
  assert(!isAgentCapablePane(mkPane()), "plain shell is not agent-capable");
  assert(!canMakeOrchestrator(mkPane()), "plain shell cannot be made orchestrator");
});

test("eligible: PTY agent Tab", () => {
  const pane = mkPane({ agent_type: "claude" });
  assert(isAgentCapablePane(pane), "agent_type marks agent-capable");
  assert(canMakeOrchestrator(pane), "agent Tab is eligible");
});

test("eligible: chatBackend Tab", () => {
  const pane = mkPane({
    backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "s1" },
  });
  assert(isAgentCapablePane(pane), "chat_backend marks agent-capable");
  assert(canMakeOrchestrator(pane), "chatBackend Tab is eligible");
});

test("ineligible: already orchestrator", () => {
  assert(
    !canMakeOrchestrator(mkPane({ agent_type: "claude", role: "Orchestrator", can_spawn: true })),
    "orchestrator blocked",
  );
});

test("eligible: worker can atomically transition out of hierarchy", () => {
  assert(
    canMakeOrchestrator(
      mkPane({ agent_type: "claude", role: "Worker", parent_pane_id: "orch-1" }),
    ),
    "nested worker can become an orchestrator",
  );
});

test("ineligible: protected Tab", () => {
  assert(
    !canMakeOrchestrator(mkPane({ agent_type: "claude", protected: true })),
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
