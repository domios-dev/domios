import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import plugin, { forward, parseOf } from "../src/plugin";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = JSON.parse(readFileSync(join(root, "plugin.json"), "utf8"));

describe("browser plugin onAgentCommand", () => {
  it("forwards the verb and its args to the caller's own surface as a JSON array", () => {
    const call = vi.fn(() => JSON.stringify({ result: "clicked @e3" }));
    expect(forward({ sessionId: "tab-1", verb: "click", args: ["@e3"] }, call)).toEqual({ result: "clicked @e3" });
    expect(call).toHaveBeenCalledWith("click", '["@e3"]');
  });

  it("passes host refusals through and treats a missing verb as help", () => {
    const call = vi.fn(() => JSON.stringify({ error: "no browser open" }));
    expect(forward({ sessionId: "tab-1", verb: "", args: [] }, call)).toEqual({ error: "no browser open" });
    expect(call).toHaveBeenCalledWith("help", "[]");
  });

  it("parses --of out of any position and passes it to the core beside the remaining words", () => {
    expect(parseOf(["@e3", "--of", "Snake"])).toEqual({ args: ["@e3"], of: "Snake" });
    expect(parseOf(["--of=QA", "button", "Sign in"])).toEqual({ args: ["button", "Sign in"], of: "QA" });
    expect(parseOf(["@e3", "note"])).toEqual({ args: ["@e3", "note"] });
    expect(parseOf(["--of"])).toEqual({ error: expect.stringContaining("--of needs") });
    expect(parseOf(["--of", " "])).toHaveProperty("error");
    expect(parseOf(["--of=", "x"])).toHaveProperty("error");
    expect(parseOf(["--of", "A", "--of", "B"])).toEqual({ error: "--of can name only one agent" });

    const call = vi.fn(() => JSON.stringify({ result: "clicked @e3" }));
    expect(forward({ sessionId: "tab-qa", verb: "click", args: ["@e3", "--of", "Snake"] }, call)).toEqual({ result: "clicked @e3" });
    expect(call).toHaveBeenCalledWith("click", '{"args":["@e3"],"of":"Snake"}');
  });

  it("refuses a malformed --of without calling the core", () => {
    const call = vi.fn(() => "{}");
    expect(forward({ sessionId: "tab-qa", verb: "annotate", args: ["@e3", "x", "--of"] }, call)).toHaveProperty("error");
    expect(call).not.toHaveBeenCalled();
  });

  it("declares annotate and --of in its verb list", () => {
    expect(manifest.agentVerbs.some((verb: string) => verb.startsWith("annotate "))).toBe(true);
    expect(manifest.agentVerbs.some((verb: string) => verb.includes("--of"))).toBe(true);
  });

  it("reports an unusable host answer instead of inventing a result", () => {
    expect(forward({ sessionId: "s", verb: "status", args: [] }, () => "not json")).toHaveProperty("error");
    expect(forward({ sessionId: "s", verb: "status", args: [] }, () => "{}")).toHaveProperty("error");
    expect(forward({ sessionId: "s", verb: "status", args: [] }, undefined)).toHaveProperty("error");
    expect(typeof plugin.onAgentCommand).toBe("function");
  });

  it("declares a stream surface on the browser runtime with the grants its verbs need", () => {
    const [surface] = manifest.capabilities.surfaces;
    expect(surface).toMatchObject({ renderer: "stream", runtime: "browser", agentBound: true });
    expect(manifest.capabilities.view).toBeUndefined();
    expect(manifest.permissions).toEqual({ agentCommands: true, surfaces: true });
    expect(manifest.agentKeywords.length).toBeLessThanOrEqual(10);
    expect(readFileSync(join(root, manifest.agentGuide), "utf8").length).toBeLessThan(16 * 1024);
  });
});
