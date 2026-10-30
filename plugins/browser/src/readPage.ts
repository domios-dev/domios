import type { BlueprintJobUpdate, BlueprintStepRequest } from "@codeterm/plugin-sdk";
import { redactCredentials } from "./redaction";

type SurfaceCall = (verb: string, argsJson: string) => string;
type Reply = { result: string } | { error: string };
export type ReadPageOutcome = "read" | "navigation_failed" | "needs_human" | "browser_busy";

const DEFAULT_LINES = 200;
const MAX_LINES = 2000;
// The `text` verb limits characters; this budget keeps the step payload under its 64 KiB bound.
const TEXT_CHARS = 48_000;
const TEXT_BYTES = 48_000;
const TRUNCATED = "\n[truncated]";
// A verb can block for its 15 s wait bound; never start one the step deadline cannot cover.
const VERB_MARGIN_MS = 16_000;
// Core answers `status` itself, without touching any runtime, when the owner has no browser.
const NO_BROWSER = /^no browser open\b/;

function ask(call: SurfaceCall, verb: string, args: string[]): Reply {
  try {
    const reply = JSON.parse(call(verb, JSON.stringify(args))) as { result?: unknown; error?: unknown };
    if (typeof reply?.error === "string") return { error: reply.error };
    if (typeof reply?.result === "string") return { result: reply.result };
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

function boundText(text: string, lines: number): string {
  const kept = text.split("\n");
  let out = kept.slice(0, lines).join("\n");
  let truncated = kept.length > lines;
  if (utf8Bytes(out) > TEXT_BYTES) {
    // Cut whole code points: a lone surrogate would make the host reject the reply.
    while (utf8Bytes(out) > TEXT_BYTES - TRUNCATED.length) {
      const points = Array.from(out);
      out = points.slice(0, Math.floor(points.length * 0.9)).join("");
    }
    truncated = true;
  }
  return truncated ? out + TRUNCATED : out;
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
export function readPage(request: BlueprintStepRequest, call: SurfaceCall | undefined, now: () => number = Date.now): BlueprintJobUpdate {
  const deadline = now() + Math.max(0, Number(request.deadlineMs) || 0);
  const finish = (outcome: ReadPageOutcome, text: string, finalUrl = ""): BlueprintJobUpdate => ({
    job: request.context.operation,
    state: "succeeded",
    outcome,
    outputs: {
      text: [{ kind: "text", text: redactCredentials(text) }],
      final_url: [{ kind: "text", text: redactCredentials(finalUrl) }],
    },
  });
  const url = requestedUrl(request);
  if (!call) return finish("navigation_failed", "host.surface is unavailable in this Domios build");
  if (!url || url.length > 2048 || /\s/.test(url) || !/^https?:\/\//i.test(url)) {
    return finish("navigation_failed", "read_page needs one http(s) URL from the url input or the url param");
  }
  const probe = ask(call, "status", []);
  if ("error" in probe) return finish("navigation_failed", probe.error);
  if (!NO_BROWSER.test(probe.result)) return finish("browser_busy", "the run owner already has a browser open; read_page leaves it untouched");
  const covered = () => deadline - now() > VERB_MARGIN_MS;
  try {
    const opened = ask(call, "open", []);
    if ("error" in opened) return finish("navigation_failed", opened.error);
    if (!covered()) return finish("navigation_failed", "the step deadline cannot cover navigation", url);
    const moved = ask(call, "goto", [url]);
    if ("error" in moved) return finish("navigation_failed", moved.error, url);
    const waitText = request.params?.wait_text;
    if (typeof waitText === "string" && waitText.trim()) {
      if (!covered()) return finish("navigation_failed", "the step deadline cannot cover wait_text", url);
      const waited = ask(call, "wait", ["--text", waitText]);
      if ("error" in waited) return finish("navigation_failed", waited.error, url);
    }
    const status = ask(call, "status", []);
    if ("error" in status) return finish("navigation_failed", status.error, url);
    const page = statusField(status.result, "page") ?? "";
    const separator = page.lastIndexOf(" — ");
    const finalUrl = separator >= 0 ? page.slice(separator + 3).trim() : url;
    const needs = statusField(status.result, "needs human");
    if (needs && needs !== "none") return finish("needs_human", `the page needs a human: ${needs}`, finalUrl);
    const read = ask(call, "text", [String(TEXT_CHARS)]);
    if ("error" in read) return finish("navigation_failed", read.error, finalUrl);
    return finish("read", boundText(read.result, lineLimit(request.params?.limit)), finalUrl);
  } finally {
    ask(call, "close", []);
  }
}
