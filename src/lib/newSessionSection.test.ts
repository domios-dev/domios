import { describe, expect, it } from "vitest";
import { allowedNewSessionModes, choiceForSessionSection, type NewSessionChoice } from "./newSessionChoice";

describe("new session section policy", () => {
  it("allows only agents in General and agents or Teams in a workspace", () => {
    expect(allowedNewSessionModes("general")).toEqual(["single"]);
    expect(allowedNewSessionModes("workspace")).toEqual(["single", "pair"]);
  });

  it("converts a remembered Team to its agent selection for General", () => {
    const team: NewSessionChoice = { mode: "pair", orchestratorProviderId: "lead", orchestratorModelId: "model", orchestratorEffortId: "high", plannerProviderId: "peer" };
    expect(choiceForSessionSection(team, "general")).toEqual({ mode: "single", providerId: "lead", modelId: "model", effortId: "high" });
    expect(choiceForSessionSection(team, "workspace")).toBe(team);
  });

  it("preserves a single agent including its preset in either section", () => {
    const agent: NewSessionChoice = { mode: "single", providerId: "provider", modelId: "model", effortId: "high", preset: "preset" };
    expect(choiceForSessionSection(agent, "general")).toBe(agent);
    expect(choiceForSessionSection(agent, "workspace")).toBe(agent);
  });
});
