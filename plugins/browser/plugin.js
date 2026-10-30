"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// plugins/browser/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default,
  forward: () => forward,
  parseOf: () => parseOf
});
module.exports = __toCommonJS(plugin_exports);

// plugins/browser/src/redaction.ts
var CREDENTIALS = /* @__PURE__ */ new Set(["token", "access_token", "api_token", "key", "secret", "password"]);
function redactCredentials(text) {
  return text.replace(
    /(^|[?#&;\s"'<>`()\[\]{}\\])([^=?#&;\s"'<>`()\[\]{}\\]+)=([^?#&;\s"'<>`()\[\]{}\\]*)/g,
    (match, prefix, name) => {
      let decoded;
      try {
        decoded = decodeURIComponent(name.replace(/\+/g, " ")).toLowerCase();
      } catch {
        return match;
      }
      return CREDENTIALS.has(decoded) ? `${prefix}${name}=REDACTED` : match;
    }
  );
}

// plugins/browser/src/readPage.ts
var DEFAULT_LINES = 200;
var MAX_LINES = 2e3;
var TEXT_CHARS = 48e3;
var TEXT_BYTES = 48e3;
var TRUNCATED = "\n[truncated]";
var VERB_MARGIN_MS = 16e3;
var NO_BROWSER = /^no browser open\b/;
function ask(call, verb, args) {
  try {
    const reply = JSON.parse(call(verb, JSON.stringify(args)));
    if (typeof reply?.error === "string") return { error: reply.error };
    if (typeof reply?.result === "string") return { result: reply.result };
  } catch {
  }
  return { error: `the browser runtime returned no answer for ${verb}` };
}
function utf8Bytes(text) {
  let bytes = 0;
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    bytes += code < 128 ? 1 : code < 2048 ? 2 : code < 65536 ? 3 : 4;
  }
  return bytes;
}
function boundText(text, lines) {
  const kept = text.split("\n");
  let out = kept.slice(0, lines).join("\n");
  let truncated = kept.length > lines;
  if (utf8Bytes(out) > TEXT_BYTES) {
    while (utf8Bytes(out) > TEXT_BYTES - TRUNCATED.length) {
      const points = Array.from(out);
      out = points.slice(0, Math.floor(points.length * 0.9)).join("");
    }
    truncated = true;
  }
  return truncated ? out + TRUNCATED : out;
}
function statusField(status, name) {
  const line = status.split("\n").find((entry) => entry.startsWith(`${name}: `));
  return line?.slice(name.length + 2).trim();
}
function requestedUrl(request) {
  const bound = request.inputs?.url?.[0];
  const raw = bound && bound.kind === "text" ? bound.text : request.params?.url;
  return typeof raw === "string" && raw.trim() ? raw.trim() : void 0;
}
function lineLimit(raw) {
  return Number.isInteger(raw) && raw >= 1 ? Math.min(raw, MAX_LINES) : DEFAULT_LINES;
}
function readPage(request, call, now = Date.now) {
  const deadline = now() + Math.max(0, Number(request.deadlineMs) || 0);
  const finish = (outcome, text, finalUrl = "") => ({
    job: request.context.operation,
    state: "succeeded",
    outcome,
    outputs: {
      text: [{ kind: "text", text: redactCredentials(text) }],
      final_url: [{ kind: "text", text: redactCredentials(finalUrl) }]
    }
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
    const separator = page.lastIndexOf(" \u2014 ");
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

// plugins/browser/src/plugin.ts
function hostSurfaceCall() {
  return typeof host === "object" && host && host.surface ? host.surface.call : void 0;
}
var OF_USAGE = "--of needs an agent name or tab id";
function parseOf(raw) {
  const args = [];
  let of;
  for (let index = 0; index < raw.length; index += 1) {
    const word = String(raw[index]);
    let value;
    if (word === "--of") {
      index += 1;
      value = index < raw.length ? String(raw[index]) : "";
    } else if (word.startsWith("--of=")) {
      value = word.slice("--of=".length);
    } else {
      args.push(word);
      continue;
    }
    if (!value.trim()) return { error: OF_USAGE };
    if (of !== void 0) return { error: "--of can name only one agent" };
    of = value.trim();
  }
  return of === void 0 ? { args } : { args, of };
}
function forward(ctx, call = hostSurfaceCall()) {
  if (!call) return { error: "host.surface is unavailable in this Domios build" };
  if (ctx.verb === "connect-secret") return { error: "connect accepts its token only through --stdin" };
  const secret = ctx.verb === "connect" ? ctx.body?.trim() : ctx.body;
  const verb = secret !== void 0 && ctx.verb === "type" ? "type-secret" : ctx.verb === "connect" ? "connect-secret" : ctx.verb === "attach" ? "open" : ctx.verb || "help";
  if (secret !== void 0 && !["type", "connect"].includes(ctx.verb)) return { error: "Browser body input is supported only by type and connect" };
  if (ctx.verb === "connect" && !secret?.trim()) return { error: "connect requires --stdin with a nonempty token" };
  const parsed = parseOf(Array.isArray(ctx.args) ? ctx.args : []);
  if ("error" in parsed) return { error: parsed.error };
  const redact = (text) => redactCredentials(secret ? text.split(secret).join("REDACTED") : text);
  if (ctx.verb === "connect") {
    try {
      const opening = JSON.parse(call("open", JSON.stringify(parsed.of === void 0 ? parsed.args : { args: parsed.args, of: parsed.of })));
      if (typeof opening.error === "string") return { error: redact(opening.error) };
      if (typeof opening.result !== "string") return { error: "the browser runtime did not confirm the requested session" };
    } catch {
      return { error: "the browser runtime could not open the requested session" };
    }
  }
  if (secret !== void 0) parsed.args.push(secret);
  const payload = parsed.of === void 0 ? parsed.args : { args: parsed.args, of: parsed.of };
  let reply;
  try {
    reply = JSON.parse(call(verb, JSON.stringify(payload)));
  } catch {
    return { error: `the browser runtime returned no answer for ${verb}` };
  }
  if (reply && typeof reply === "object") {
    const { result, error } = reply;
    if (typeof error === "string") return { error: redact(error) };
    if (typeof result === "string") return { result: redact(result) };
  }
  return { error: `the browser runtime returned no answer for ${verb}` };
}
var plugin = {
  onAgentCommand: (ctx) => forward(ctx),
  blueprintSubmit: (request) => readPage(request, hostSurfaceCall())
};
var plugin_default = plugin;
