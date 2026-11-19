import { ioResolve, type HostIoSteps, type BlueprintJobUpdate, type BlueprintOutputValue, type BlueprintStepRequest } from "@codeterm/plugin-sdk";
import { redactCredentials } from "./redaction";

type SurfaceCall = (verb: string, argsJson: string) => string | HostIoSteps<string>;
type SurfaceIdentity = { tab: string; instance: string };
type Reply = { result: string; surface?: SurfaceIdentity; captured_at?: string } | { error: string };
export type ReadPageOutcome = "read" | "navigation_failed" | "needs_human" | "browser_busy" | "profile_not_allowed" | "origin_not_allowed";
type SnapshotItem = { role: string; name: string; depth: number; href?: string; value?: string };
type SnapshotValue = { items: SnapshotItem[]; truncated: boolean; error?: string };
type NamedPageReply = {
  outcome: ReadPageOutcome;
  text: string;
  final_url: string;
  profile: string;
  captured_at?: string;
};

const DEFAULT_LINES = 200;
const MAX_LINES = 2000;
const MAX_SNAPSHOT_ITEMS = 256;
const TEXT_CHARS = 48_000;
// The Blueprint execution envelope is capped at 32 KiB, including URL and port metadata.
const TEXT_BYTES = 23_000;
const TRUNCATED = "\n[truncated]";
// A verb can block for its 15 s wait bound; never start one the step deadline cannot cover.
const VERB_MARGIN_MS = 16_000;
// Core answers `status` itself, without touching any runtime, when the owner has no browser.
const NO_BROWSER = /^no browser open\b/;
const CAPTURE_TIMESTAMP = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;

function* ask(call: SurfaceCall, verb: string, args: string[], surface?: SurfaceIdentity): HostIoSteps<Reply> {
  try {
    const reply = JSON.parse(yield* ioResolve(call(verb, JSON.stringify(surface ? { args, surface } : args)))) as { result?: unknown; error?: unknown; surface?: SurfaceIdentity; captured_at?: string };
    if (typeof reply?.error === "string") return { error: reply.error };
    if (typeof reply?.result === "string") return { result: reply.result, surface: reply.surface, captured_at: reply.captured_at };
  } catch { /* an unparseable answer is reported below */ }
  return { error: `the browser runtime returned no answer for ${verb}` };
}

// QuickJS has no TextEncoder; count UTF-8 bytes per code point.
function utf8Bytes(text: string): number {
  let bytes = 0;
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    bytes += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
  }
  return bytes;
}

function boundText(text: string, lines: number, budget = TEXT_BYTES): string {
  const kept = text.split("\n");
  let out = kept.slice(0, lines).join("\n");
  let truncated = kept.length > lines;
  if (utf8Bytes(JSON.stringify(out)) > budget) {
    // Cut whole code points: a lone surrogate would make the host reject the reply.
    while (utf8Bytes(JSON.stringify(out + TRUNCATED)) > budget) {
      const points = Array.from(out);
      out = points.slice(0, Math.floor(points.length * 0.9)).join("");
    }
    truncated = true;
  }
  return truncated ? out + TRUNCATED : out;
}

function boundSnapshot(text: string, limit: number): SnapshotValue {
  const snapshot = JSON.parse(text) as { items?: SnapshotItem[]; truncated?: boolean };
  if (!Array.isArray(snapshot.items)) throw new Error("the browser returned an invalid accessibility snapshot");
  const items: SnapshotItem[] = [];
  let bytes = 64;
  for (const { role, name, depth, href, value } of snapshot.items) {
    const item = { role, name, depth, href, value };
    bytes += utf8Bytes(JSON.stringify(item)) + 1;
    if (items.length >= limit || bytes > TEXT_BYTES) break;
    items.push(item);
  }
  return { items, truncated: snapshot.truncated === true || items.length < snapshot.items.length };
}

function statusField(status: string, name: string): string | undefined {
  const line = status.split("\n").find((entry) => entry.startsWith(`${name}: `));
  return line?.slice(name.length + 2).trim();
}

function requestedUrl(request: BlueprintStepRequest): string | undefined {
  const bound = request.inputs?.url?.[0];
  const raw = bound && bound.kind === "text" ? bound.text : request.params?.url;
  return typeof raw === "string" && raw.trim() ? raw.trim() : undefined;
}

function lineLimit(raw: unknown): number {
  return Number.isInteger(raw) && (raw as number) >= 1 ? Math.min(raw as number, MAX_LINES) : DEFAULT_LINES;
}

/** Reads one page in a browser the run opens and always closes; an owner browser that is already open is never touched. */
export function* readPage(request: BlueprintStepRequest, call: SurfaceCall | undefined, now: () => number = Date.now): HostIoSteps<BlueprintJobUpdate> {
  const snapshot = request.nodeType === "browser.snapshot_page";
  const limit = Math.min(lineLimit(request.params?.limit), snapshot ? MAX_SNAPSHOT_ITEMS : MAX_LINES);
  const deadline = now() + Math.max(0, Number(request.deadlineMs) || 0);
  const finish = (outcome: ReadPageOutcome, text: string, finalUrl = "", profile = "", capturedAt = ""): BlueprintJobUpdate => {
    if (outcome === "read" && (typeof capturedAt !== "string" || !CAPTURE_TIMESTAMP.test(capturedAt))) {
      outcome = "navigation_failed";
      text = "the browser returned no valid capture timestamp";
    }
    const safe = redactCredentials(text);
    let content: BlueprintOutputValue = { kind: "text", text: boundText(safe, outcome === "read" ? limit : MAX_LINES) };
    if (snapshot) {
      try {
        const value = outcome === "read"
          ? boundSnapshot(safe, limit)
          : { items: [], truncated: false, error: boundText(safe, MAX_LINES) };
        content = { kind: "json", value: { ...value } };
      } catch {
        outcome = "navigation_failed";
        content = { kind: "json", value: { items: [], truncated: false, error: "the browser returned an invalid accessibility snapshot" } };
      }
    }
    return {
    job: request.context.operation,
    state: "succeeded",
    outcome,
    outputs: {
      [snapshot ? "snapshot" : "text"]: [content],
      final_url: [{ kind: "text", text: boundText(redactCredentials(finalUrl), 1, 8000) }],
      profile: [{ kind: "text", text: profile }],
      capture_id: [{ kind: "text", text: outcome === "read" ? request.context.operation : "" }],
      captured_at: [{ kind: "text", text: outcome === "read" ? capturedAt : "" }],
    },
    };
  };
  const url = requestedUrl(request);
  if (!call) return finish("navigation_failed", "host.surface is unavailable in this Domios build");
  if (!url || url.length > 2048 || /\s/.test(url) || !/^https?:\/\//i.test(url)) {
    return finish("navigation_failed", "read_page needs one http(s) URL from the url input or the url param");
  }
  if (request.params?.profile !== undefined) {
    const reply = yield* ask(call, "read_named_page", []);
    if ("error" in reply) return finish("navigation_failed", reply.error);
    try {
      const captured = JSON.parse(reply.result) as NamedPageReply;
      return finish(captured.outcome, captured.text, captured.final_url, captured.profile, captured.captured_at);
    } catch {
      return finish("navigation_failed", "the browser runtime returned an invalid named-profile read");
    }
  }
  const probe = yield* ask(call, "status", []);
  if ("error" in probe) return finish("navigation_failed", probe.error);
  if (!NO_BROWSER.test(probe.result)) return finish("browser_busy", "the step already has a surface open; read_page leaves it untouched");
  const covered = () => deadline - now() > VERB_MARGIN_MS;
  let surface: SurfaceIdentity | undefined;
  try {
    const opened = yield* ask(call, "open", []);
    if ("error" in opened) return finish("navigation_failed", opened.error);
    surface = opened.surface;
    if (!surface || typeof surface.tab !== "string" || typeof surface.instance !== "string") return finish("navigation_failed", "the host did not grant a run-owned surface");
    if (!covered()) return finish("navigation_failed", "the step deadline cannot cover navigation", url);
    const moved = yield* ask(call, "goto", [url], surface);
    if ("error" in moved) return finish("navigation_failed", moved.error, url);
    const waitText = request.params?.wait_text;
    if (typeof waitText === "string" && waitText.trim()) {
      if (!covered()) return finish("navigation_failed", "the step deadline cannot cover wait_text", url);
      const waited = yield* ask(call, "wait", ["--text", waitText], surface);
      if ("error" in waited) return finish("navigation_failed", waited.error, url);
    }
    const status = yield* ask(call, "status", [], surface);
    if ("error" in status) return finish("navigation_failed", status.error, url);
    const page = statusField(status.result, "page") ?? "";
    const separator = page.lastIndexOf(" — ");
    const finalUrl = separator >= 0 ? page.slice(separator + 3).trim() : url;
    const needs = statusField(status.result, "needs human");
    if (needs && needs !== "none") return finish("needs_human", `the page needs a human: ${needs}`, finalUrl);
    const read = yield* ask(call, snapshot ? "snapshot" : "text",
      snapshot ? [String(limit), "--with-text", "--json"] : [String(TEXT_CHARS)], surface);
    if ("error" in read) return finish("navigation_failed", read.error, finalUrl);
    return finish("read", read.result, finalUrl, statusField(status.result, "profile") ?? "", read.captured_at);
  } finally {
    if (surface) yield* ask(call, "close", [], surface);
  }
}
