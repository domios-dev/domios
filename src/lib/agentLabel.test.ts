import { describe, expect, finish, it } from "../test/standaloneTest";
import { agentBracketLabel, classifyAgentLiveness, type AgentLiveness } from "./agentLabel";

interface LabelRow {
  row: string;
  provider: string | null;
  model: string | null;
  liveness: AgentLiveness;
  expected: string | null;
}

const LABEL_ROWS: LabelRow[] = [
  { row: "fresh spawn", provider: "claude", model: "opus", liveness: "live", expected: "claude:opus" },
  { row: "restoring", provider: "claude", model: "opus", liveness: "unobserved", expected: "claude:opus?" },
  { row: "restored as shell", provider: "claude", model: "opus", liveness: "absent", expected: "claude:opus!" },
  { row: "orchestrator", provider: "claude", model: "opus", liveness: "live", expected: "claude:opus" },
  { row: "planner", provider: "claude", model: "opus", liveness: "live", expected: "claude:opus" },
  { row: "worker", provider: "codex", model: "gpt-5", liveness: "live", expected: "codex:gpt-5" },
  { row: "general agent", provider: "claude", model: "opus", liveness: "live", expected: "claude:opus" },
  { row: "model unobserved", provider: "codex", model: null, liveness: "live", expected: "codex" },
  { row: "blank model", provider: "claude", model: "   ", liveness: "live", expected: "claude" },
  { row: "plain terminal", provider: null, model: "opus", liveness: "absent", expected: null },
];

const LAUNCH_IN_FLIGHT_STAGES = [
  "agent_launching",
  "launch_written",
  "preparing_role_instructions",
  "delivering_instructions",
];

describe("classifyAgentLiveness", () => {
  it("never calls a record live", () => {
    // The live reproduction: role intact, nonce matched, agent_state idle,
    // boot_stage shell_ready, no agent process.
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: true, bootStage: "shell_ready" }),
    ).toBe("absent");
  });

  it("treats a tab with no PTY yet as unobserved, not absent", () => {
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: null, bootStage: "shell_ready" }),
    ).toBe("unobserved");
  });

  it.each(LAUNCH_IN_FLIGHT_STAGES)("treats %s as a launch still in flight", (bootStage: string) => {
    expect(classifyAgentLiveness({ declaresAgent: true, ptyLive: true, bootStage })).toBe(
      "unobserved",
    );
  });

  it("treats an explicit booting flag as unobserved", () => {
    expect(
      classifyAgentLiveness({
        declaresAgent: true,
        ptyLive: true,
        bootStage: "shell_ready",
        bootInFlight: true,
      }),
    ).toBe("unobserved");
  });

  it.each(["provider_ready", "agent_ready"])("calls %s live", (bootStage: string) => {
    expect(classifyAgentLiveness({ declaresAgent: true, ptyLive: true, bootStage })).toBe("live");
  });

  it("calls a failed spawn absent", () => {
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: true, bootStage: "spawn_failed" }),
    ).toBe("absent");
  });

  it("proves nothing about the agent from a re-attached PTY", () => {
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: true, bootStage: "pty_reattached" }),
    ).toBe("unobserved");
  });

  it("lets a dead PTY override the record", () => {
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: false, bootStage: "agent_ready" }),
    ).toBe("absent");
  });

  it("has no agent when the tab declares none", () => {
    expect(
      classifyAgentLiveness({ declaresAgent: false, ptyLive: true, bootStage: "agent_ready" }),
    ).toBe("absent");
  });

  it("reads chatBackend liveness from the plugin session", () => {
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: null, chatSessionOpen: true }),
    ).toBe("live");
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: null, chatSessionOpen: false }),
    ).toBe("absent");
  });
});

describe("agentBracketLabel", () => {
  it.each(LABEL_ROWS)("follows one rule for every row", (row: LabelRow) => {
    expect(agentBracketLabel(row.provider, row.model, row.liveness)).toBe(row.expected);
  });

  it.each(["manager", "worker", "general_agent", "orchestrator", "planner"])(
    "never carries the %s role",
    (role: string) => {
      expect(agentBracketLabel("claude", "opus", "live")).not.toContain(role);
    },
  );
});

finish();
