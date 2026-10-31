import type { BlueprintStepRequest } from "@codeterm/plugin-sdk";
import { describe, expect, it } from "vitest";
import plugin from "../src/plugin";
import { readPage } from "../src/readPage";

type Answer = { result: string } | { error: string };

const STATUS = "owner: Owner\nprofile: temporary\npage: Example Domain — https://example.com/landing\nneeds human: none";

function fakeSurface(answers: Partial<Record<string, Answer>> = {}) {
  const calls: { verb: string; args: string[] }[] = [];
  let opened = false;
  const call = (verb: string, argsJson: string) => {
    const payload = JSON.parse(argsJson);
    calls.push({ verb, args: Array.isArray(payload) ? payload : payload.args });
    if (verb !== "open" && opened) expect(payload.surface).toEqual({ tab: "run-surface", instance: "run-instance" });
    if (verb === "open") opened = true;
    const answer = answers[verb] ?? (verb === "status" && !opened ? { result: "no browser open; run `open` first" }
      : verb === "status" ? { result: STATUS }
        : verb === "text" ? { result: "Example Domain\nsecond line" }
          : { result: `${verb} ok` });
    return JSON.stringify(verb === "open" && !("error" in answer) ? { ...answer, surface: { tab: "run-surface", instance: "run-instance" } } : answer);
  };
  return { call, calls, verbs: () => calls.map((entry) => entry.verb) };
}

function request(params: Record<string, unknown>, url?: string): BlueprintStepRequest {
  return {
    context: { scope: { workspace: "w", team: null, run: "r" }, owner: "owner-tab", node: "read", attempt: "a", operation: "op-1", nodeLabel: "Browser: read page", inputLineage: [] },
    nodeType: "browser.read_page",
    revision: 1,
    params,
    inputs: url === undefined ? {} : { url: [{ kind: "text", text: url }] },
    job: null,
    deadlineMs: 60_000,
  };
}

const texts = (update: ReturnType<typeof readPage>) => ({
  text: (update.outputs.text[0] as { text: string }).text,
  finalUrl: (update.outputs.final_url[0] as { text: string }).text,
});

describe("browser.read_page Blueprint step", () => {
  it("never closes a foreign browser when open fails", () => {
    const surface = fakeSurface({ open: { error: "open refused" } });
    expect(readPage(request({ url: "https://example.com" }), surface.call).outcome).toBe("navigation_failed");
    expect(surface.verbs()).toEqual(["status", "open"]);
  });

  it("uses only the surface identity granted by open after a UI browser races the probe", () => {
    const surface = fakeSurface();
    const update = readPage(request({ url: "https://example.com" }), surface.call);
    expect(update.outcome).toBe("read");
    expect(surface.verbs()).toContain("goto");
  });
  it("opens, navigates, checks status, reads and closes in order and settles synchronously", () => {
    const surface = fakeSurface();
    const update = readPage(request({ url: "https://example.com" }), surface.call);
    expect(surface.verbs()).toEqual(["status", "open", "goto", "status", "text", "close"]);
    expect(surface.calls[2].args).toEqual(["https://example.com"]);
    expect(update).toMatchObject({ job: "op-1", state: "succeeded", outcome: "read" });
    expect(texts(update)).toEqual({ text: "Example Domain\nsecond line", finalUrl: "https://example.com/landing" });
  });

  it("waits for wait_text, keeps limit lines and lets a bound url input win", () => {
    const surface = fakeSurface();
    const update = readPage(request({ url: "https://ignored.example", wait_text: "Ready", limit: 1 }, "https://bound.example"), surface.call);
    expect(surface.verbs()).toEqual(["status", "open", "goto", "wait", "status", "text", "close"]);
    expect(surface.calls[2].args).toEqual(["https://bound.example"]);
    expect(surface.calls[3].args).toEqual(["--text", "Ready"]);
    expect(texts(update).text).toBe("Example Domain\n[truncated]");
  });

  it("bounds multibyte page text under the step payload with whole code points", () => {
    const surface = fakeSurface({ text: { result: "😀".repeat(20_000) } });
    const { text } = texts(readPage(request({ url: "https://example.com" }), surface.call));
    expect(text.endsWith("\n[truncated]")).toBe(true);
    expect(Buffer.byteLength(text, "utf8")).toBeLessThanOrEqual(48_000);
    expect(text.replace("\n[truncated]", "")).toMatch(/^(😀)+$/u);
  });

  it("redacts credentials in the page text and the final url", () => {
    const fixtureToken = ["opaque", "s3cret"].join("-");
    const surface = fakeSurface({ text: { result: `callback?token=${fixtureToken}&page=2` } });
    const call = (verb: string, args: string) => verb === "status" && surface.calls.some((c) => c.verb === "goto")
      ? (surface.calls.push({ verb, args: JSON.parse(args) }), JSON.stringify({ result: STATUS.replace("/landing", `/landing?access_token=${fixtureToken}`) }))
      : surface.call(verb, args);
    const update = readPage(request({ url: "https://example.com" }), call);
    expect(texts(update)).toEqual({ text: "callback?token=REDACTED&page=2", finalUrl: "https://example.com/landing?access_token=REDACTED" });
    expect(JSON.stringify(update)).not.toContain("opaque-s3cret");
  });

  it("maps a goto error to navigation_failed and still closes", () => {
    const surface = fakeSurface({ goto: { error: "net::ERR_NAME_NOT_RESOLVED" } });
    const update = readPage(request({ url: "https://missing.invalid" }), surface.call);
    expect(update.outcome).toBe("navigation_failed");
    expect(texts(update)).toEqual({ text: "net::ERR_NAME_NOT_RESOLVED", finalUrl: "https://missing.invalid" });
    expect(surface.verbs()).toEqual(["status", "open", "goto", "close"]);
  });

  it("reports needs_human from status without reading the page", () => {
    const surface = fakeSurface();
    const call = (verb: string, args: string) => verb === "status" && surface.calls.some((c) => c.verb === "goto")
      ? (surface.calls.push({ verb, args: JSON.parse(args) }), JSON.stringify({ result: STATUS.replace("needs human: none", "needs human: captcha") }))
      : surface.call(verb, args);
    const update = readPage(request({ url: "https://example.com" }), call);
    expect(update.outcome).toBe("needs_human");
    expect(surface.verbs()).toEqual(["status", "open", "goto", "status", "close"]);
  });

  it("leaves an already open owner browser untouched with browser_busy", () => {
    const surface = fakeSurface({ status: { result: STATUS } });
    const update = readPage(request({ url: "https://example.com" }), surface.call);
    expect(update.outcome).toBe("browser_busy");
    expect(surface.verbs()).toEqual(["status"]);
  });

  it("refuses non-http urls and missing urls before touching any browser", () => {
    for (const params of [{}, { url: "file:///etc/passwd" }, { url: "https://a b" }]) {
      const surface = fakeSurface();
      expect(readPage(request(params), surface.call).outcome).toBe("navigation_failed");
      expect(surface.calls).toEqual([]);
    }
  });

  it("closes without navigating when the deadline cannot cover a verb", () => {
    const surface = fakeSurface();
    const update = readPage({ ...request({ url: "https://example.com" }), deadlineMs: 1_000 }, surface.call);
    expect(update.outcome).toBe("navigation_failed");
    expect(surface.verbs()).toEqual(["status", "open", "close"]);
  });

  it("is exported as the plugin's only step entry point", () => {
    expect(typeof plugin.blueprintSubmit).toBe("function");
    expect(plugin.blueprintReconcile).toBeUndefined();
    expect(plugin.blueprintCancel).toBeUndefined();
  });
});
