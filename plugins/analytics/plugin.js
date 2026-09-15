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

// plugins/analytics/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default,
  runQuery: () => runQuery
});
module.exports = __toCommonJS(plugin_exports);
function hostAnalytics() {
  return typeof host === "object" && host ? host.analytics : void 0;
}
function isSnapshot(value) {
  const candidate = value;
  return typeof candidate.scope === "object" && typeof candidate.recording === "object" && typeof candidate.tokens === "object";
}
function runQuery(query, analytics = hostAnalytics()) {
  if (!analytics) return { kind: "unavailable" };
  if (!query || typeof query !== "object" || Array.isArray(query)) {
    return { kind: "refused", message: "query must be an object" };
  }
  const answer = analytics.query(query);
  if (!answer || typeof answer !== "object") return { kind: "unavailable" };
  const refusal = answer;
  if (typeof refusal.error === "string") {
    return { kind: "refused", message: typeof refusal.message === "string" ? refusal.message : refusal.error };
  }
  return isSnapshot(answer) ? { kind: "snapshot", snapshot: answer } : { kind: "unavailable" };
}
function viewCall(method, args) {
  if (method !== "query") return { kind: "refused", message: `unknown method: ${method}` };
  return runQuery(args?.query);
}
var plugin = {
  viewCall
};
var plugin_default = plugin;
