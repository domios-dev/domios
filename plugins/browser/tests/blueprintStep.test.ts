import { ioTestExport } from "../../provider-shared/tests/hostIo";
import type { BlueprintStepRequest } from "@codeterm/plugin-sdk";
import { describe, expect, it } from "vitest";
import plugin from "../src/plugin";
import { readPage as readPageSteps } from "../src/readPage";
const {readPage} = ioTestExport({readPage: readPageSteps});

type Answer = { result: string } | { error: string };

const STATUS = "owner: Owner\nprofile: temporary\npage: Example Domain — https://example.com/landing\nneeds human: none";
const CAPTURED_AT = "2026-11-19T07:23:04.005Z";

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
    if ("error" in answer) return JSON.stringify(answer);
    if (verb === "open") return JSON.stringify({ ...answer, surface: { tab: "run-surface", instance: "run-instance" } });
    if (verb === "text" || verb === "snapshot") return JSON.stringify({ ...answer, captured_at: CAPTURED_AT });
    return JSON.stringify(answer);
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
    expect(update.outputs.profile).toEqual([{ kind: "text", text: "temporary" }]);
    expect(update.outputs.capture_id).toEqual([{ kind: "text", text: "op-1" }]);
    expect(update.outputs.captured_at).toEqual([{ kind: "text", text: CAPTURED_AT }]);
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
    expect(Buffer.byteLength(text, "utf8")).toBeLessThanOrEqual(23_000);
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

  it("keeps an allowlisted capture's profile, redirect URL and native operation identity", () => {
    const req = request({ url: "https://example.com", profile: "Work" });
    const call = (verb: string, args: string) => {
      expect(verb).toBe("read_named_page");
      expect(JSON.parse(args)).toEqual([]);
      return JSON.stringify({ result: JSON.stringify({
        outcome: "read", text: "Example Domain", final_url: "https://example.com/landing", profile: "Work", captured_at: CAPTURED_AT,
      }) });
    };
    const first = readPage(req, call);
    const second = readPage({ ...req, context: { ...req.context, operation: "op-2" } }, call);
    expect(texts(first)).toEqual({ text: "Example Domain", finalUrl: "https://example.com/landing" });
    expect(first.outputs.profile).toEqual([{ kind: "text", text: "Work" }]);
    expect(first.outputs.capture_id).toEqual([{ kind: "text", text: "op-1" }]);
    expect(first.outputs.captured_at).toEqual([{ kind: "text", text: CAPTURED_AT }]);
    expect(second.outputs.capture_id).toEqual([{ kind: "text", text: "op-2" }]);
  });

  it.each([
    ["profile_not_allowed", "The owner must allow the named profile in plugins.browser.blueprint_profiles"],
    ["browser_busy", "browser profile is in use"],
    ["origin_not_allowed", "The owner must allow this origin for the named profile in plugins.browser.blueprint_origins"],
  ])("preserves a typed %s refusal and never falls back to a temporary profile", (outcome, text) => {
    const surface = fakeSurface({
      read_named_page: { result: JSON.stringify({ outcome, text, final_url: "", profile: "" }) },
    });
    const update = readPage(request({ url: "https://example.com", profile: "Work" }), surface.call);
    expect(update.outcome).toBe(outcome);
    expect(texts(update)).toEqual({ text, finalUrl: "" });
    expect(update.outputs.capture_id).toEqual([{ kind: "text", text: "" }]);
    expect(update.outputs.captured_at).toEqual([{ kind: "text", text: "" }]);
    expect(surface.verbs()).toEqual(["read_named_page"]);
  });

  it("reads a bounded accessibility snapshot through the same temporary surface lifecycle", () => {
    const link = { role: "link", name: "Post", depth: 1, href: "https://example.com/post" };
    const surface = fakeSurface({ snapshot: { result: JSON.stringify({
      items: [link, { role: "heading", name: "Example Domain", depth: 1 }],
      truncated: false,
    }) } });
    const req = request({ url: "https://example.com", limit: 1 });
    const update = readPage({ ...req, nodeType: "browser.snapshot_page" }, surface.call);
    expect(surface.verbs()).toEqual(["status", "open", "goto", "status", "snapshot", "close"]);
    expect(surface.calls[4].args).toEqual(["1", "--with-text", "--json"]);
    expect(update.outputs.snapshot).toEqual([{ kind: "json", value: { items: [link], truncated: true } }]);
    expect(update.outputs.capture_id).toEqual([{ kind: "text", text: "op-1" }]);
    expect(update.outputs.captured_at).toEqual([{ kind: "text", text: CAPTURED_AT }]);
  });

  it("keeps a named-profile snapshot's structured control values and link targets", () => {
    const items = [
      { role: "link", name: "Post", depth: 1, href: "https://example.com/post" },
      { role: "spinbutton", name: "Count", depth: 1, value: "7" },
    ];
    const surface = fakeSurface({ read_named_page: { result: JSON.stringify({
      outcome: "read",
      profile: "Work",
      final_url: "https://example.com",
      captured_at: CAPTURED_AT,
      text: JSON.stringify({ items, truncated: false }),
    }) } });
    const req = request({ profile: "Work" }, "https://example.com");
    const update = readPage({ ...req, nodeType: "browser.snapshot_page" }, surface.call);
    expect(update.outputs.snapshot).toEqual([{ kind: "json", value: { items, truncated: false } }]);
    expect(update.outputs.captured_at).toEqual([{ kind: "text", text: CAPTURED_AT }]);
  });

  it("requires a host capture timestamp for a successful read", () => {
    const surface = fakeSurface();
    const call = (verb: string, args: string) => verb === "text"
      ? JSON.stringify({ result: "Example Domain" })
      : surface.call(verb, args);
    const update = readPage(request({ url: "https://example.com" }), call);
    expect(update.outcome).toBe("navigation_failed");
    expect(update.outputs.captured_at).toEqual([{ kind: "text", text: "" }]);
  });

  it("is exported as the plugin's only step entry point", () => {
    expect(typeof plugin.blueprintSubmit).toBe("function");
    expect(plugin.blueprintReconcile).toBeUndefined();
    expect(plugin.blueprintCancel).toBeUndefined();
  });
});
