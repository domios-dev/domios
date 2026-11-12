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

// plugins/chat-preprocessor/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default,
  preprocessText: () => preprocessText
});
module.exports = __toCommonJS(plugin_exports);

// packages/plugin-sdk/src/ioPrograms.ts
function* ioValue(request) {
  return yield request;
}
function defineProviderIo(source, io) {
  const exports = { ...source };
  for (const key of Object.keys(source)) {
    const value = source[key];
    if (typeof value !== "function") continue;
    exports[key] = function(...args) {
      const result = value.apply(source, args);
      return result && typeof result.next === "function" ? io().run(result) : result;
    };
  }
  return exports;
}

// plugins/chat-preprocessor/src/plugin.ts
var SYSTEM_MEM_RECALL_ID = "mem-recall";
function currentHost() {
  return typeof host === "object" && host ? host : {};
}
function readSettings() {
  const raw = currentHost().settingsJson?.();
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}
function asString(value, fallback = "") {
  return typeof value === "string" ? value : fallback;
}
function asNumber(value, fallback, min, max) {
  const n = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, Math.min(max, Math.floor(n)));
}
function asPosition(value) {
  return value === "append" ? "append" : "prepend";
}
function compactSnippet(text, maxChars) {
  const oneLine = text.replace(/\s+/g, " ").trim();
  if (oneLine.length <= maxChars) return oneLine;
  return `${oneLine.slice(0, Math.max(0, maxChars - 1)).trimEnd()}\u2026`;
}
function formatMemBlock(text, settings, overrides, hits = []) {
  const k = asNumber(overrides?.k ?? settings.memK, 3, 1, 8);
  const maxChars = asNumber(overrides?.maxChars ?? settings.memMaxChars, 140, 40, 500);
  const rows = hits.filter((hit) => typeof hit.id === "string" && typeof hit.text === "string").slice(0, k).map((hit) => `- ${hit.id}: ${compactSnippet(String(hit.text), maxChars)}`);
  if (rows.length === 0) return "";
  return `Found similar in mem:
${rows.join("\n")}`;
}
function applyPosition(text, block, position) {
  if (!block.trim()) return text;
  return position === "append" ? `${text}

${block}` : `${block}

${text}`;
}
function userPresets(settings) {
  return Array.isArray(settings.presets) ? settings.presets.filter((p) => p && typeof p === "object") : [];
}
function findPreset(settings) {
  const active = asString(settings.activePreset, SYSTEM_MEM_RECALL_ID).trim() || SYSTEM_MEM_RECALL_ID;
  if (active === SYSTEM_MEM_RECALL_ID) return { kind: "mem-recall", config: {} };
  const preset = userPresets(settings).find((p) => asString(p.id).trim() === active);
  if (!preset) return null;
  const kind = asString(preset.kind, "template") === "mem-recall" ? "mem-recall" : "template";
  const config = preset.config && typeof preset.config === "object" && !Array.isArray(preset.config) ? preset.config : {};
  return { kind, preset, config };
}
function renderTemplate(template, text, memBlock) {
  return template.replaceAll("{{text}}", text).replaceAll("{{mem}}", memBlock);
}
function preprocessText(text, settings, hits = []) {
  if (settings.enabled !== true) return null;
  const selected = findPreset(settings);
  if (!selected) return null;
  if (selected.kind === "mem-recall") {
    const block = formatMemBlock(text, settings, selected.config, hits);
    return applyPosition(text, block, asPosition(selected.config.position ?? settings.insertPosition));
  }
  const template = asString(selected.preset?.template, "{{text}}");
  const memBlock = template.includes("{{mem}}") ? formatMemBlock(text, settings, selected.config, hits) : "";
  return renderTemplate(template, text, memBlock);
}
var plugin = {
  meta: {
    id: "chat-preprocessor",
    displayName: "Chat Preprocessor"
  },
  match: "text",
  *chatPreprocess(ctx) {
    const text = asString(ctx?.text);
    if (!text.trim()) return null;
    const settings = readSettings();
    const selected = settings.enabled === true ? findPreset(settings) : null;
    const needsMem = selected && (selected.kind === "mem-recall" || asString(selected.preset?.template).includes("{{mem}}"));
    let hits = [];
    if (needsMem) {
      const k = asNumber(selected.config.k ?? settings.memK, 3, 1, 8);
      try {
        hits = (yield* ioValue({ op: "memSearch", options: { query: text, k } }))?.hits ?? [];
      } catch {
        return null;
      }
    }
    const next = preprocessText(text, settings, hits);
    return next === null || next === text ? null : { text: next };
  }
};
var plugin_default = defineProviderIo(plugin, () => host.io);
