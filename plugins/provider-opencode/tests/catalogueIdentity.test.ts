import { describe, expect, it } from "vitest";
import { parseOpencodeModels, sessionModelIdentity } from "../src/helpers";

describe("OpenCode catalogue and session identity", () => {
  it("offers only CLI fixture models with friendly labels", () => {
    const models = parseOpencodeModels('opencode/big-pickle\n{"name":"Big Pickle"}\nopencode/ling-3.1-flash-free\n{"name":"Ling"}\nopenai/text-embedding-3\n');
    expect(models?.map((model) => model.id)).toEqual(["opencode/big-pickle", "opencode/ling-3.1-flash-free"]);
    expect(models?.[0].displayName).toBe("OpenCode Zen Big Pickle");
    expect(models?.some((model) => model.id === "opencode/deepseek-v4-flash-free")).toBe(false);
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
