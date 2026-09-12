import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, finish, it } from "../test/standaloneTest";
import { agentBracketLabel, classifyAgentLiveness, type AgentLiveness } from "./agentLabel";
import { shortModelName } from "./agentModelName";

interface LabelRow {
  row: string;
  provider: string | null;
  model: string | null;
  liveness: AgentLiveness;
  expected: string | null;
}

const LABEL_ROWS: LabelRow[] = [
  // a recognised model renders alone: the model replaces the provider
  { row: "fresh spawn", provider: "claude", model: "opus", liveness: "live", expected: "opus" },
  { row: "restoring", provider: "claude", model: "opus", liveness: "unobserved", expected: "opus?" },
  { row: "restored as shell", provider: "claude", model: "opus", liveness: "absent", expected: "opus!" },
  { row: "codex codename", provider: "codex", model: "gpt-5.6-sol", liveness: "live", expected: "sol" },
  { row: "codex codename restoring", provider: "codex", model: "gpt-5.6-luna", liveness: "unobserved", expected: "luna?" },
  { row: "codex codename gone", provider: "codex", model: "gpt-5.3-codex-spark", liveness: "absent", expected: "spark!" },
  { row: "uncodenamed codex", provider: "codex", model: "gpt-5.5", liveness: "live", expected: "gpt 5.5" },
  { row: "long vendor id", provider: "claude", model: "claude-sonnet-4-5-20250929", liveness: "live", expected: "sonnet" },
  // no model: the provider stands in, at every liveness
  { row: "model unobserved", provider: "codex", model: null, liveness: "live", expected: "codex" },
  { row: "model unobserved, restoring", provider: "codex", model: null, liveness: "unobserved", expected: "codex?" },
  { row: "model unobserved, gone", provider: "codex", model: null, liveness: "absent", expected: "codex!" },
  { row: "blank model", provider: "claude", model: "   ", liveness: "live", expected: "claude" },
  // a model no catalogue claims: the provider stands in
  { row: "unrecognised model", provider: "lmstudio", model: "nvidia/nemotron-3-nano", liveness: "live", expected: "lmstudio" },
  { row: "unrecognised model, gone", provider: "lmstudio", model: "some-local-gguf", liveness: "absent", expected: "lmstudio!" },
  // no usable provider: no bracket, whatever the model says
  { row: "plain terminal", provider: null, model: "opus", liveness: "absent", expected: null },
  { row: "blank provider", provider: "  ", model: "opus", liveness: "live", expected: null },
];

const LIVENESS_STATES: AgentLiveness[] = ["live", "unobserved", "absent"];

const RECOGNISED_PAIRS: Array<[string, string]> = [
  ["claude", "claude-opus-5"],
  ["codex", "gpt-5.6-terra"],
];

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

/** The determinism guarantee: a new catalogue entry fails until its family is listed. */
function catalogueIds(): string[] {
  const ids = ["claude", "codex"].flatMap((provider) => {
    const manifest = JSON.parse(
      readFileSync(path.join(REPO_ROOT, "plugins", `provider-${provider}`, "plugin.json"), "utf8"),
    );
    return (manifest.models as Array<{ id: string }>).map((model) => model.id);
  });
  if (ids.length === 0) throw new Error("provider catalogues are the source of truth and are empty");
  return ids;
}

const CATALOGUE_IDS = catalogueIds();

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

  it("proves nothing about the agent from an elapsed launch deadline", () => {
    expect(
      classifyAgentLiveness({
        declaresAgent: true,
        ptyLive: true,
        bootStage: "launch_deadline_elapsed",
      }),
    ).toBe("unobserved");
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

  it.each(LIVENESS_STATES)("never joins the provider to a recognised model when %s", (liveness: AgentLiveness) => {
    for (const [provider, model] of RECOGNISED_PAIRS) {
      const label = agentBracketLabel(provider, model, liveness) as string;
      expect(label).not.toContain(":");
      expect(label).not.toContain(provider);
    }
  });

  it("never lets a long vendor id reach the bracket verbatim", () => {
    for (const [provider, model] of [
      ["claude", "claude-sonnet-4-5-20250929"],
      ["codex", "gpt-5.6-sol"],
      ["lmstudio", "google/gemma-4-26b-a4b-qat"],
    ]) {
      expect(agentBracketLabel(provider, model, "live")).not.toContain(model);
    }
  });

  it("chooses the provider without erasing the stored model", () => {
    const tab = { agent_type: "lmstudio", agent_session_model: "mystery-local-build" };
    expect(agentBracketLabel(tab.agent_type, tab.agent_session_model, "live")).toBe("lmstudio");
    expect(tab.agent_session_model).toBe("mystery-local-build");
  });
});

describe("shortModelName", () => {
  it.each(CATALOGUE_IDS)("resolves the shipped catalogue id %s", (id: string) => {
    const short = shortModelName(id);
    expect(typeof short).toBe("string");
    expect((short as string).length > 0).toBe(true);
  });

  it("mirrors the Rust families", () => {
    expect(shortModelName("gpt-5.6-terra")).toBe("terra");
    expect(shortModelName("gpt-6-astra")).toBe("astra");
    expect(shortModelName("gpt-5.4-mini")).toBe("mini");
    expect(shortModelName("Opus")).toBe("opus");
    expect(shortModelName("claude-fable-5-1")).toBe("fable");
    expect(shortModelName("openai/gpt-5.5")).toBe("gpt 5.5");
  });

  it("resolves a bumped version to the same family", () => {
    expect(shortModelName("gpt-5.9-sol")).toBe(shortModelName("gpt-5.6-sol"));
    expect(shortModelName("claude-opus-9")).toBe(shortModelName("opus"));
  });

  it.each(["", "   ", "nvidia/nemotron-3-nano", "some-local-gguf", "codex", "gpt"])(
    "reports %s as unrecognised",
    (id: string) => {
      expect(shortModelName(id)).toBe(null);
    },
  );
});

finish();
