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
  forward: () => forward
});
module.exports = __toCommonJS(plugin_exports);
function hostSurfaceCall() {
  return typeof host === "object" && host && host.surface ? host.surface.call : void 0;
}
function forward(ctx, call = hostSurfaceCall()) {
  if (!call) return { error: "host.surface is unavailable in this Domios build" };
  const verb = ctx.verb || "help";
  let reply;
  try {
    reply = JSON.parse(call(verb, JSON.stringify(Array.isArray(ctx.args) ? ctx.args : [])));
  } catch {
    return { error: `the browser runtime returned no answer for ${verb}` };
  }
  if (reply && typeof reply === "object") {
    const { result, error } = reply;
    if (typeof error === "string") return { error };
    if (typeof result === "string") return { result };
  }
  return { error: `the browser runtime returned no answer for ${verb}` };
}
var plugin = {
  onAgentCommand: (ctx) => forward(ctx)
};
var plugin_default = plugin;
