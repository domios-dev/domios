import { describe, expect, it } from "vitest";
import manifest from "../plugin.json";
import { parseOpencodeModels, sessionModelIdentity } from "../src/helpers";
import { isFreeZenModel } from "../src/freeModel";

describe("OpenCode catalogue and session identity", () => {
  it("offers only CLI fixture models with friendly labels and leads with free Zen models", () => {
    const models = parseOpencodeModels('opencode/big-pickle\n{"name":"Big Pickle"}\nopencode/ling-3.1-flash-free\n{"name":"Ling"}\nopenai/text-embedding-3\n');
    expect(models?.map((model) => model.id)).toEqual(["opencode/ling-3.1-flash-free", "opencode/big-pickle"]);
    expect(models?.[0].badge).toBe("free");
    expect(models?.[1].displayName).toBe("OpenCode Zen Big Pickle");
    expect(models?.some((model) => model.id === "opencode/deepseek-v4-flash-free")).toBe(false);
  });
  it("selects a zero-auth free Zen model first from a live alphabetical catalogue", () => {
    const stdout = [
      "anthropic/claude-fable-5",
      "anthropic/claude-opus-4-8",
      "deepseek/deepseek-v4-pro",
      "opencode/beam-free",
      "opencode/big-pickle",
      "opencode/deepseek-v4.1-flash-free",
      "opencode/gpt-5.5",
    ].join("\n");
    const models = parseOpencodeModels(stdout);
    expect(models?.map((model) => model.id)).toEqual([
      "opencode/beam-free",
      "opencode/deepseek-v4.1-flash-free",
      "anthropic/claude-fable-5",
      "anthropic/claude-opus-4-8",
      "deepseek/deepseek-v4-pro",
      "opencode/big-pickle",
      "opencode/gpt-5.5",
    ]);
    expect(models?.[0].badge).toBe("free");
    expect(models?.[1].badge).toBe("free");
    expect(models?.[2].badge).toBeUndefined();
  });
  it("chooses free Zen models by cost and tool-calling capability when verbose metadata is present", () => {
    const stdout = [
      'anthropic/free-looking-free\n{"cost":{"input":0,"output":0}}',
      'opencode/paid-but-named-free\n{"cost":{"input":0.5,"output":1.5}}',
      'opencode/zen-chat-only\n{"cost":{"input":0,"output":0},"capabilities":{"toolcall":false}}',
      'opencode/zen-capable\n{"cost":{"input":0,"output":0},"capabilities":{"toolcall":true}}',
    ].join("\n");
    const models = parseOpencodeModels(stdout);
    expect(models?.map((model) => model.id)).toEqual([
      "opencode/zen-capable",
      "opencode/zen-chat-only",
      "anthropic/free-looking-free",
      "opencode/paid-but-named-free",
    ]);
    expect(isFreeZenModel("opencode/zen-capable", { cost: { input: 0, output: 0 } })).toBe(true);
    expect(isFreeZenModel("opencode/paid-but-named-free", { cost: { input: 0.5, output: 1.5 } })).toBe(false);
    expect(isFreeZenModel("anthropic/free-looking-free", { cost: { input: 0, output: 0 } })).toBe(false);
  });
  it("ships a current static fallback whose default is a free Zen model", () => {
    expect(manifest.badge).toBe("free tokens");
    expect(manifest.defaultModel).toBeTruthy();
    expect(isFreeZenModel(manifest.defaultModel)).toBe(true);
    expect(manifest.models[0]?.id).toBe(manifest.defaultModel);
    expect(manifest.models.some((model) => model.id === "opencode/deepseek-v4-flash-free")).toBe(false);
    expect(manifest.models.some((model) => model.id === "opencode/deepseek-v4.1-flash-free")).toBe(true);
  });
  it("bounds malformed and oversized output", () => {
    expect(parseOpencodeModels("error: failed")).toBeNull();
    expect(parseOpencodeModels("x".repeat(1_048_577))).toBeNull();
  });
  it("normalizes qualified and split identities to the same session model", () => {
    const split = { role: "assistant", providerID: "opencode", modelID: "big-pickle" };
    expect(sessionModelIdentity(split)).toBe("opencode/big-pickle");
    expect(sessionModelIdentity({ ...split, modelID: "opencode/big-pickle" })).toBe(sessionModelIdentity(split));
    expect(sessionModelIdentity({ role: "assistant", modelID: "big-pickle" })).toBeNull();
    expect(sessionModelIdentity({ ...split, role: "user" })).toBeNull();
  });
});
