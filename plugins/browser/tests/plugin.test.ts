import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import plugin, { forward, parseOf } from "../src/plugin";
import { redactCredentials } from "../src/redaction";
import redactionCases from "./redaction-cases.json";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = JSON.parse(readFileSync(join(root, "plugin.json"), "utf8"));

describe("Browser stdin typing", () => {
  it("delivers the separate body without echoing it in successes or failures", () => {
    const secret = "opaque-body-λ\"\n";
    for (const field of ["result", "error"] as const) {
      const call = vi.fn((_verb: string, payload: string) => {
        expect(JSON.parse(payload)).toEqual(["textbox", "Password", secret]);
        return JSON.stringify({ [field]: `field value: ${secret}` });
      });
      expect(forward({ sessionId: "stdin", verb: "type", args: ["textbox", "Password"], body: secret }, call))
        .toEqual({ [field]: "field value: REDACTED" });
      expect(call.mock.calls[0][0]).toBe("type-secret");
    }
  });

  it("keeps owner routing and rejects bodies on unrelated verbs", () => {
    const call = vi.fn(() => JSON.stringify({ result: "typed" }));
    forward({ sessionId: "stdin", verb: "type", args: ["@e3", "--of", "QA"], body: "opaque-body" }, call);
    expect(call).toHaveBeenCalledWith("type-secret", JSON.stringify({ args: ["@e3", "opaque-body"], of: "QA" }));
    call.mockClear();
    expect(forward({ sessionId: "stdin", verb: "goto", args: [], body: "opaque-body" }, call)).toHaveProperty("error");
    expect(call).not.toHaveBeenCalled();
  });
});

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


describe("Browser credential redaction", () => {
  it.each(redactionCases)("redacts $input", ({ input, expected }) => {
    expect(redactCredentials(input)).toBe(expected);
    expect(redactCredentials(expected)).toBe(expected);
  });

  it.each(["snapshot", "goto", "status", "text", "wait", "inbox", "open", "back", "forward", "reload", "click"])("redacts %s successes and failures without changing navigation input", (verb) => {
    const url = "https://example.test/#token=opaque";
    for (const field of ["result", "error"] as const) {
      const call = vi.fn(() => JSON.stringify({ [field]: url }));
      expect(forward({ sessionId: "s", verb, args: [url] }, call)).toEqual({ [field]: "https://example.test/#token=REDACTED" });
      expect(call).toHaveBeenCalledWith(verb, JSON.stringify([url]));
    }
  });
});

describe("Browser named instance connection", () => {
  it("opens with public options and sends the stdin token only through the secret verb", () => {
    const secret = "opaque-session-token";
    const args = ["--profile", "qa", "--origin", "http://localhost:7686"];
    const call = vi.fn((verb: string, payload: string) => {
      if (verb === "open") { expect(JSON.parse(payload)).toEqual(args); expect(payload).not.toContain(secret); return JSON.stringify({ result: "opened" }); }
      expect(verb).toBe("connect-secret");
      expect(JSON.parse(payload)).toEqual([...args, secret]);
      return JSON.stringify({ error: `unconfirmed ${secret}` });
    });
    expect(forward({ sessionId: "qa", verb: "connect", args, body: secret }, call)).toEqual({ error: "unconfirmed REDACTED" });
    expect(call).toHaveBeenCalledTimes(2);
  });
  it("refuses missing stdin and stops when opening fails", () => {
    const call = vi.fn(() => JSON.stringify({ error: "origin_mismatch" }));
    expect(forward({ sessionId: "qa", verb: "connect", args: [] }, call)).toHaveProperty("error");
    expect(call).not.toHaveBeenCalled();
    expect(forward({ sessionId: "qa", verb: "connect", args: [], body: "opaque" }, call)).toEqual({ error: "origin_mismatch" });
    expect(call).toHaveBeenCalledTimes(1);
  });
  it("attaches through the existing owned session open verb", () => {
    const call = vi.fn((_verb: string, _payload: string) => JSON.stringify({ result: "opened" }));
    forward({ sessionId: "qa", verb: "attach", args: ["--profile", "qa", "--origin", "http://localhost:7686"] }, call);
    expect(call.mock.calls[0][0]).toBe("open");
  });
});
