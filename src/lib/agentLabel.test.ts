import { describe, expect, it } from "vitest";
import { agentBracketLabel, classifyAgentLiveness, type AgentLiveness } from "./agentLabel";

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

  it("treats a launch in flight as unobserved", () => {
    for (const bootStage of [
      "agent_launching",
      "launch_written",
      "preparing_role_instructions",
      "delivering_instructions",
    ]) {
      expect(classifyAgentLiveness({ declaresAgent: true, ptyLive: true, bootStage })).toBe(
        "unobserved",
      );
    }
    expect(
      classifyAgentLiveness({
        declaresAgent: true,
        ptyLive: true,
        bootStage: "shell_ready",
        bootInFlight: true,
      }),
    ).toBe("unobserved");
  });

  it("calls a proven-up provider live and a failed spawn absent", () => {
    for (const bootStage of ["provider_ready", "agent_ready"]) {
      expect(classifyAgentLiveness({ declaresAgent: true, ptyLive: true, bootStage })).toBe("live");
    }
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: true, bootStage: "spawn_failed" }),
    ).toBe("absent");
  });

  it("proves nothing about the agent from a re-attached PTY", () => {
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: true, bootStage: "pty_reattached" }),
    ).toBe("unobserved");
  });

  it("lets a dead PTY and a missing agent_type override the record", () => {
    expect(
      classifyAgentLiveness({ declaresAgent: true, ptyLive: false, bootStage: "agent_ready" }),
    ).toBe("absent");
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
  const rows: Array<{
    row: string;
    provider: string | null;
    model: string | null;
    liveness: AgentLiveness;
    expected: string | null;
  }> = [
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

  it.each(rows)("follows one rule for $row", ({ provider, model, liveness, expected }) => {
    expect(agentBracketLabel(provider, model, liveness)).toBe(expected);
  });

  it("never carries a role", () => {
    const label = agentBracketLabel("claude", "opus", "live");
    for (const role of ["manager", "worker", "general_agent", "orchestrator", "planner"]) {
      expect(label).not.toContain(role);
    }
  });
});
