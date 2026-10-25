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
  const secret = ctx.body;
  const verb = secret !== void 0 && ctx.verb === "type" ? "type-secret" : ctx.verb || "help";
  if (secret !== void 0 && ctx.verb !== "type") return { error: "Browser body input is supported only by type" };
  const parsed = parseOf(Array.isArray(ctx.args) ? ctx.args : []);
  if ("error" in parsed) return { error: parsed.error };
  if (secret !== void 0) parsed.args.push(secret);
  const payload = parsed.of === void 0 ? parsed.args : { args: parsed.args, of: parsed.of };
  const redact = (text) => redactCredentials(secret ? text.split(secret).join("REDACTED") : text);
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
  onAgentCommand: (ctx) => forward(ctx)
};
var plugin_default = plugin;
