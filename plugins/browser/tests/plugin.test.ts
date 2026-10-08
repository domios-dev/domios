import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import plugin, { forward } from "../src/plugin";

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
