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

// plugins/provider-acp/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default,
  onAgentCommand: () => onAgentCommand,
  readProfileLimits: () => readProfileLimits2,
  runtimePlan: () => runtimePlan,
  runtimeReduce: () => runtimeReduce,
  runtimeResponse: () => runtimeResponse
});
module.exports = __toCommonJS(plugin_exports);

// plugins/provider-acp/src/auth.ts
function decodeAuthMethods(value) {
  if (value === void 0) return [];
  if (!Array.isArray(value) || value.length > 32) throw new Error("Invalid ACP authMethods");
  const ids = /* @__PURE__ */ new Set();
  return value.map((raw) => {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) throw new Error("Invalid ACP auth method");
    const method = raw;
    if (typeof method.id !== "string" || !method.id.length || method.id.length > 128 || /[\u0000-\u001f]/.test(method.id) || typeof method.name !== "string" || !method.name.length || method.name.length > 256 || ids.has(method.id)) throw new Error("Invalid ACP auth method");
    ids.add(method.id);
    return { id: method.id, name: method.name, type: method.type === void 0 || method.type === "agent" ? "agent" : method.type === "terminal" ? "terminal" : "unsupported" };
  });
}
function authenticationState(methods2, evidence = {}) {
  if (!methods2) return "unknown";
  if (evidence.authenticated) return "authenticated";
  if (evidence.required || evidence.keyPresent === false) return "needs_auth";
  if (methods2.length && methods2.every((method) => method.type !== "agent")) return "unavailable";
  return "unknown";
}
function agentAuthMethod(methods2, id) {
  const method = methods2.find((method2) => method2.id === id && method2.type === "agent");
  if (!method) throw new Error("Authentication method was not declared or is unavailable");
  return method;
}

// plugins/provider-acp/src/runtimeProtocol.ts
var protocolVersion = 1;
var methods = {
  initialize: "initialize",
  newSession: "session/new",
  load: "session/load",
  resume: "session/resume",
  prompt: "session/prompt",
  cancel: "session/cancel",
  update: "session/update"
};

// plugins/provider-acp/src/runtimeReduceTools.ts
var object = (value) => value !== null && typeof value === "object" && !Array.isArray(value) ? value : {};
var text = (value) => typeof value === "string" && value.length > 0 ? value : void 0;
var canonicalJson = (value) => JSON.stringify(value, (_key, item) => item !== null && typeof item === "object" && !Array.isArray(item) ? Object.fromEntries(Object.keys(item).sort().map((key) => [key, item[key]])) : item);
function diff(content) {
  if (typeof content.path !== "string" || typeof content.newText !== "string" || content.oldText !== null && typeof content.oldText !== "string") return void 0;
  const lines = (value) => value ? value.replace(/\n$/, "").split("\n") : [];
  const oldLines = lines(typeof content.oldText === "string" ? content.oldText : "");
  const newLines = lines(content.newText);
  return {
    path: content.path,
    kind: content.oldText === null ? "add" : "modify",
    additions: newLines.length,
    deletions: oldLines.length,
    hunks: [{
      oldStart: oldLines.length ? 1 : 0,
      oldLines: oldLines.length,
      newStart: newLines.length ? 1 : 0,
      newLines: newLines.length,
      lines: [...oldLines.map((line) => `-${line}`), ...newLines.map((line) => `+${line}`)]
    }]
  };
}
function reduceTool(update, previous, seq) {
  const id = text(update.toolCallId);
  if (!id) return void 0;
  const row = { ...previous, id, seq: previous?.seq ?? seq, type: "tool_call", content: "", toolId: id, collapsed: true };
  if (typeof update.title === "string") {
    row.toolTitle = update.title;
    row.toolName = update.title;
  }
  if (typeof update.kind === "string") {
    const kinds2 = { execute: "command", edit: "edit", read: "read", search: "search" };
    row.toolKind = Object.prototype.hasOwnProperty.call(kinds2, update.kind) ? kinds2[update.kind] : "generic";
  }
  if (update.rawInput !== void 0) {
    row.toolInput = update.rawInput;
    row.toolArgs = typeof update.rawInput === "string" ? update.rawInput : canonicalJson(update.rawInput);
  }
  const statuses = { pending: "running", in_progress: "running", completed: "completed", failed: "failed", cancelled: "interrupted" };
  const status = Object.prototype.hasOwnProperty.call(statuses, String(update.status)) ? statuses[String(update.status)] : void 0;
  if (status) {
    row.toolStatus = status;
    row.toolError = status === "failed";
  }
  if (update.rawOutput !== void 0) row.toolResult = typeof update.rawOutput === "string" ? update.rawOutput : canonicalJson(update.rawOutput);
  if (Array.isArray(update.content)) {
    const results = [];
    const diffs = [];
    for (const raw of update.content) {
      const part = object(raw);
      const block = object(part.content);
      if (part.type === "content" && block.type === "text" && typeof block.text === "string") results.push(block.text);
      if (part.type === "diff") {
        const mapped = diff(part);
        if (mapped) diffs.push(mapped);
      }
    }
    if (results.length) row.toolResult = results.join("\n");
    if (diffs.length) row.fileDiffs = diffs;
  }
  return row;
}

// plugins/provider-acp/src/runtimeReducePlan.ts
function planRow(sessionId, update, previous, seq) {
  const entries = Array.isArray(update.entries) ? update.entries.map(object) : [];
  return {
    id: `${sessionId}:plan`,
    seq: previous?.seq ?? seq,
    type: "tool_call",
    content: "",
    toolName: "update_plan",
    toolId: `${sessionId}:plan`,
    toolInput: { plan: entries },
    toolResult: JSON.stringify(entries.map((entry) => ({ step: entry.content, status: entry.status }))),
    collapsed: true
  };
}
var count = (value) => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
function costMicros(value) {
  const cost = object(value);
  if (cost.currency !== "USD" || typeof cost.amount !== "number" || !Number.isFinite(cost.amount) || cost.amount < 0) return void 0;
  const micros = Math.round(cost.amount * 1e6);
  return count(micros) ? micros : void 0;
}
function contextUsage(threadId, value) {
  const raw = object(value);
  if (!count(raw.used) || !count(raw.size)) return void 0;
  const cost = costMicros(raw.cost);
  return { kind: "context", threadId, used_tokens: raw.used, window_tokens: raw.size, ...cost !== void 0 ? { cost_micros: cost } : {} };
}
function usage(value) {
  const raw = object(value);
  if (!count(raw.inputTokens) || !count(raw.outputTokens)) return void 0;
  const mapped = { input_tokens: raw.inputTokens, output_tokens: raw.outputTokens };
  if (count(raw.cachedReadTokens)) mapped.cache_read_tokens = raw.cachedReadTokens;
  if (count(raw.cachedWriteTokens)) mapped.cache_write_tokens = raw.cachedWriteTokens;
  if (count(raw.used)) mapped.used_tokens = raw.used;
  if (count(raw.size)) mapped.window_tokens = raw.size;
  const cost = costMicros(raw.cost);
  if (cost !== void 0) mapped.cost_micros = cost;
  return mapped;
}

// plugins/provider-acp/src/runtimeControls.ts
var object2 = (value) => value !== null && typeof value === "object" && !Array.isArray(value) ? value : {};
function label(value) {
  if (typeof value !== "string" || !value.length || value.length > 256 || /[\u0000-\u001f\u007f-\u009f]/.test(value)) return false;
  let bytes = 0;
  for (const character of value) {
    const point = character.codePointAt(0);
    if (point >= 55296 && point <= 57343) return false;
    bytes += point <= 127 ? 1 : point <= 2047 ? 2 : point <= 65535 ? 3 : 4;
  }
  return bytes <= 256;
}
function list(value, limit) {
  return Array.isArray(value) && value.length <= limit ? value.map(object2) : [];
}
function commands(value) {
  const seen = /* @__PURE__ */ new Set();
  return list(value, 128).flatMap((command) => {
    if (!label(command.name) || /[\s/]/.test(command.name) || seen.has(command.name)) return [];
    seen.add(command.name);
    const hint = object2(command.input).hint;
    return [{
      name: command.name,
      description: typeof command.description === "string" ? command.description.slice(0, 4096) : "",
      ...typeof hint === "string" ? { argumentHint: hint.slice(0, 4096) } : {}
    }];
  });
}
function configOptions(value) {
  const seen = /* @__PURE__ */ new Set();
  return list(value, 64).flatMap((config) => {
    if (!label(config.id) || !label(config.name) || seen.has(config.id)) return [];
    seen.add(config.id);
    const values = list(config.options, 128).flatMap((entry) => Array.isArray(entry.options) ? list(entry.options, 128) : [entry]);
    const ids = /* @__PURE__ */ new Set();
    const options = values.slice(0, 128).flatMap((entry) => {
      if (!label(entry.value) || !label(entry.name) || ids.has(entry.value)) return [];
      ids.add(entry.value);
      return [{ value: entry.value, name: entry.name }];
    });
    if (config.type !== "select" || !label(config.currentValue)) return [{ id: config.id, name: config.name, currentValue: String(config.currentValue).slice(0, 256) || "Unknown", options: [] }];
    return [{
      id: config.id,
      name: config.name,
      currentValue: config.currentValue,
      options,
      ...label(config.category) ? { category: config.category } : {}
    }];
  });
}
function modelOption(settings) {
  return settings?.configOptions?.find((option) => option.category === "model");
}
function effortOption(settings) {
  return settings?.configOptions?.find((option) => option.category === "thought_level");
}
function sessionModel(value, declared = []) {
  if (modelOption({ configOptions: declared })) return void 0;
  const models = object2(value), seen = /* @__PURE__ */ new Set();
  const options = list(models.availableModels, 128).flatMap((model) => {
    if (!label(model.modelId) || !label(model.name) || seen.has(model.modelId)) return [];
    seen.add(model.modelId);
    return [{ value: model.modelId, name: model.name }];
  });
  if (!label(models.currentModelId) || !seen.has(models.currentModelId)) return void 0;
  let id = "acp-session-model";
  while (declared.some((option) => option.id === id)) id += "-";
  return { id, name: "Model", category: "model", currentValue: models.currentModelId, options };
}
function selectedSessionModel(settings, id, value) {
  const option = settings?.configOptions?.find((option2) => option2.id === id);
  if (!option || !label(value) || !option.options.some((choice) => choice.value === value)) return void 0;
  return { configOptions: settings.configOptions.map((option2) => option2.id === id ? { ...option2, currentValue: value } : option2), model: value };
}
function sessionSettings(value) {
  const response = object2(value), modes = object2(response.modes);
  const seen = /* @__PURE__ */ new Set();
  const config = Array.isArray(response.configOptions) ? configOptions(response.configOptions) : void 0;
  const fallback = sessionModel(response.models, config);
  const declared = fallback ? [...(config ?? []).slice(0, 63), fallback] : config;
  const model = modelOption({ configOptions: declared })?.currentValue;
  const effort = effortOption({ configOptions: declared })?.currentValue;
  return {
    ...declared ? { configOptions: declared, ...model ? { model } : {}, ...effort ? { effort } : {} } : {},
    ...Array.isArray(modes.availableModes) ? { modes: list(modes.availableModes, 64).flatMap((mode) => {
      if (!label(mode.id) || !label(mode.name) || seen.has(mode.id)) return [];
      seen.add(mode.id);
      return [{ id: mode.id, name: mode.name }];
    }) } : {},
    ...label(modes.currentModeId) && seen.has(modes.currentModeId) ? { mode: modes.currentModeId } : {}
  };
}
function authentication(methods2, authenticated, required) {
  return { state: authenticationState(methods2, { authenticated, required }), methods: methods2.map((method) => ({ id: method.id, name: method.name, available: method.type === "agent" })) };
}

// plugins/provider-acp/src/runtimeReduceAccounting.ts
var mapKeys = ["items", "controls", "warnings", "offsetById"];
var totalsByState = /* @__PURE__ */ new WeakMap();
function utf8Length(value) {
  let bytes = 0;
  for (let index = 0; index < value.length; ) {
    const code = value.codePointAt(index);
    bytes += code <= 127 ? 1 : code <= 2047 ? 2 : code <= 65535 ? 3 : 4;
    index += code > 65535 ? 2 : 1;
  }
  return bytes;
}
function contribution(id, value) {
  if (value === void 0 || typeof value === "function" || typeof value === "symbol") return 0;
  return utf8Length(JSON.stringify({ [id]: value })) - 1;
}
function totals(state) {
  let cached = totalsByState.get(state);
  if (!cached) {
    cached = { items: 0, controls: 0, warnings: 0, offsetById: 0 };
    for (const key of mapKeys) for (const [id, value] of Object.entries(state[key])) cached[key] += contribution(id, value);
    totalsByState.set(state, cached);
  }
  return cached;
}
function copyAccounting(source, target) {
  totalsByState.set(target, { ...totals(source) });
}
function forgetAccounting(state) {
  if (state !== null && typeof state === "object") totalsByState.delete(state);
}
function mapBytes(state, key) {
  const total = totals(state)[key];
  return total ? total + 1 : 2;
}
function putEntry(state, key, id, value) {
  const cached = totals(state);
  cached[key] += contribution(id, value) - contribution(id, state[key][id]);
  state[key][id] = value;
}
function removeEntry(state, key, id) {
  const before = mapBytes(state, key), cached = totals(state);
  cached[key] -= contribution(id, state[key][id]);
  delete state[key][id];
  return mapBytes(state, key) - before;
}
function clearEntries(state, key) {
  totals(state)[key] = 0;
  state[key] = /* @__PURE__ */ Object.create(null);
}

// plugins/provider-acp/src/runtimeReduceSeen.ts
var BITS = 4096;
function bits(id) {
  return [2166136261, 374761393].map((seed) => {
    let hash = seed;
    for (let index = 0; index < id.length; index++) {
      hash ^= id.charCodeAt(index);
      hash = Math.imul(hash, 16777619) >>> 0;
    }
    return hash % BITS;
  });
}
function decode(filter) {
  const bytes = new Uint8Array(BITS / 8);
  if (filter?.length === BITS / 4) for (let index = 0; index < bytes.length; index++) bytes[index] = Number.parseInt(filter.slice(index * 2, index * 2 + 2), 16);
  return bytes;
}
function evictedMayInclude(filter, id) {
  if (filter === void 0) return false;
  const bytes = decode(filter);
  return bits(id).every((bit) => ((bytes[bit >> 3] ?? 0) & 1 << bit % 8) !== 0);
}
function recordEvicted(filter, ids) {
  if (!ids.length) return filter;
  const bytes = decode(filter);
  for (const id of ids) for (const bit of bits(id)) bytes[bit >> 3] = (bytes[bit >> 3] ?? 0) | 1 << bit % 8;
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

// plugins/provider-acp/src/runtimeReduce.ts
var OFFSET_BUDGET = 16384;
var json = (value) => value;
function initial(ctx, raw, cursor) {
  const previous = object(raw);
  if (previous.epoch !== ctx.epoch || previous.nonce !== ctx.bindingNonce || previous.sessionId !== ctx.threadId)
    return {
      epoch: ctx.epoch,
      nonce: ctx.bindingNonce,
      sessionId: ctx.threadId,
      replay: cursor.replay,
      prompt: cursor.prompt,
      turn: cursor.prompt || 1,
      ordinal: 0,
      seq: 0,
      items: /* @__PURE__ */ Object.create(null),
      offsetById: /* @__PURE__ */ Object.create(null),
      warnings: /* @__PURE__ */ Object.create(null),
      controls: /* @__PURE__ */ Object.create(null),
      evidenced: [],
      compacted: [],
      seenItems: [],
      loading: cursor.loading,
      settled: false,
      started: false
    };
  const state = previous;
  const copy = {
    ...state,
    items: Object.assign(/* @__PURE__ */ Object.create(null), state.items),
    offsetById: Object.assign(/* @__PURE__ */ Object.create(null), state.offsetById),
    warnings: Object.assign(/* @__PURE__ */ Object.create(null), state.warnings),
    controls: { ...state.controls },
    evidenced: [...state.evidenced],
    compacted: [...state.compacted],
    seenItems: [...state.seenItems],
    ...state.chunk ? { chunk: { ...state.chunk } } : {}
  };
  copyAccounting(state, copy);
  if (copy.replay !== cursor.replay) {
    copy.replay = cursor.replay;
    copy.turn = 1;
    copy.ordinal = 0;
    copy.seenItems = [];
    clearEntries(copy, "offsetById");
    delete copy.evictedMessages;
    delete copy.chunk;
    copy.settled = false;
    copy.started = false;
  } else if (copy.prompt !== cursor.prompt) {
    copy.turn = Math.max(copy.turn + 1, cursor.prompt);
    copy.ordinal = 0;
    copy.seenItems = [];
    clearEntries(copy, "offsetById");
    delete copy.evictedMessages;
    delete copy.chunk;
    copy.settled = false;
    copy.started = false;
  }
  copy.prompt = cursor.prompt;
  copy.loading = cursor.loading;
  return copy;
}
function emit(state, events, row, patch = false) {
  const previous = state.items[row.id];
  row.seq = previous?.seq ?? ++state.seq;
  if (utf8Length(JSON.stringify(row)) > 32768) {
    row = {
      id: row.id,
      seq: row.seq,
      type: row.type,
      content: "[ACP row truncated: update exceeded the display budget]",
      ...row.toolStatus ? { toolStatus: row.toolStatus } : {},
      ...row.toolId ? { toolId: row.toolId } : {}
    };
  }
  const retained = {
    ...previous,
    id: row.id,
    seq: row.seq,
    type: row.type,
    content: "",
    ...row.toolId ? { toolId: row.toolId } : {},
    ...row.toolTitle ? { toolTitle: Array.from(row.toolTitle).slice(0, 256).join("") } : {},
    ...row.toolName ? { toolName: Array.from(row.toolName).slice(0, 256).join("") } : {},
    ...row.toolKind ? { toolKind: row.toolKind } : {},
    ...row.toolStatus ? { toolStatus: row.toolStatus } : {}
  };
  if (row.toolStatus === "running" && !previous && !admitToolIdentity(state, retained)) {
    events.push({ kind: "chatItem", threadId: state.sessionId, item: { ...row, toolStatus: "interrupted", toolInterrupted: true, content: "[ACP tool tracking capacity exhausted]" } });
    return;
  }
  putEntry(state, "items", row.id, retained);
  if (!patch || JSON.stringify(previous) !== JSON.stringify(retained) || row.toolInput !== void 0 || row.toolResult !== void 0 || row.fileDiffs !== void 0 || row.toolInterrupted) events.push({ kind: patch ? "chatItemPatch" : "chatItem", threadId: state.sessionId, item: row });
}
function admitToolIdentity(state, retained) {
  let bytes = mapBytes(state, "items") + utf8Length(JSON.stringify({ [retained.id]: retained })) - 1;
  const submittedId = `${state.sessionId}:turn:${state.turn}:item:1`;
  for (const [id, row] of Object.entries(state.items)) {
    if (bytes <= 16384) break;
    if (row.toolStatus === "running" || id === state.chunk?.id || id === submittedId) continue;
    bytes += removeEntry(state, "items", id);
  }
  return bytes <= 16384;
}
function rememberOffset(state, base, cursor) {
  removeEntry(state, "offsetById", base);
  const entry = utf8Length(JSON.stringify({ [base]: cursor }));
  if (entry > OFFSET_BUDGET) return;
  let bytes = mapBytes(state, "offsetById") + entry;
  const evicted = [];
  for (const known of Object.keys(state.offsetById)) {
    if (bytes <= OFFSET_BUDGET) break;
    bytes += removeEntry(state, "offsetById", known);
    evicted.push(known);
  }
  const filter = recordEvicted(state.evictedMessages, evicted);
  if (filter !== void 0) state.evictedMessages = filter;
  putEntry(state, "offsetById", base, cursor);
}
function rememberControl(state, kind, update) {
  const encoded = JSON.stringify(update);
  if (encoded.length <= 16384) putEntry(state, "controls", kind, JSON.parse(encoded));
}
function warn(state, events, rawKind) {
  const kind = rawKind.slice(0, 128);
  if (!Object.prototype.hasOwnProperty.call(state.warnings, kind) && Object.keys(state.warnings).length >= 32) return;
  const count2 = state.warnings[kind] ?? 0;
  putEntry(state, "warnings", kind, Math.min(count2 + 1, Number.MAX_SAFE_INTEGER));
  if (!count2) events.push({ kind: "notice", threadId: state.sessionId, message: `Ignored unknown ACP session update: ${kind}` });
}
function closeChunk(ctx, state, events) {
  const chunk = state.chunk;
  if (chunk?.kind === "user_message_chunk" && !state.loading) {
    const claim = ctx.claimedSends.find((claim2) => claim2.sendId === chunk.echo && claim2.threadId === ctx.threadId && !state.evidenced.includes(claim2.sendId) && claim2.input.attachments.length === 0 && claim2.input.text.length === chunk.chars);
    if (claim) {
      if (!events.some((event) => event.kind === "chatItem" && event.item.id === chunk.id))
        events.push({ kind: "chatItem", threadId: state.sessionId, item: { ...state.items[chunk.id], content: claim.input.text } });
      state.evidenced.push(claim.sendId);
      events.push({ kind: "sendEvidence", threadId: state.sessionId, sendId: claim.sendId, turnId: ctx.activeTurnId ?? `${state.sessionId}:turn:${state.turn}`, itemId: chunk.id });
    }
  }
  delete state.chunk;
}
function settleStream(ctx, raw, reason) {
  const source = object(raw);
  if (!ctx.threadId) return { state: raw, events: [] };
  if (source.sessionId !== ctx.threadId || source.epoch !== ctx.epoch || source.nonce !== ctx.bindingNonce)
    return { state: raw, events: [{ kind: "turnState", threadId: ctx.threadId, state: "idle", ...reason === "cancelled" ? { detail: "interrupted" } : {} }] };
  const state = source;
  if (state.settled) return { state: raw, events: [] };
  const next = { ...state, items: Object.assign(/* @__PURE__ */ Object.create(null), state.items), offsetById: Object.assign(/* @__PURE__ */ Object.create(null), state.offsetById), controls: { ...state.controls }, warnings: Object.assign(/* @__PURE__ */ Object.create(null), state.warnings), evidenced: [...state.evidenced], settled: true };
  copyAccounting(state, next);
  const events = [];
  closeChunk(ctx, next, events);
  for (const row of Object.values(next.items)) if (row.toolStatus === "running")
    emit(next, events, { id: row.id, seq: row.seq, type: row.type, content: "", ...row.toolId ? { toolId: row.toolId } : {}, toolStatus: "interrupted", toolInterrupted: true }, true);
  events.push({ kind: "turnState", threadId: ctx.threadId, turnId: `${state.sessionId}:turn:${state.turn}`, state: "idle", ...reason === "cancelled" ? { detail: "interrupted" } : {} });
  boundState(next);
  return { state: json(next), events };
}
function clientRequest(method) {
  return ["session/request_permission", "fs/read_text_file", "fs/write_text_file", "terminal/create", "terminal/output", "terminal/wait_for_exit", "terminal/kill", "terminal/release"].includes(method);
}
function startTurn(ctx, state, events) {
  if (!state.loading && !state.settled && state.prompt > 0 && !state.started && !ctx.activeTurnId) {
    state.started = true;
    events.push({ kind: "turnState", threadId: state.sessionId, turnId: `${state.sessionId}:turn:${state.turn}`, state: "working" });
  }
}
function submittedRow(state, events, submitted, evidenced) {
  const id = `${state.sessionId}:turn:${state.turn}:item:1`;
  if (!state.items[id]) {
    state.ordinal = Math.max(state.ordinal, 1);
    emit(state, events, { id, seq: 0, type: "user", content: submitted.input.text });
  }
  if (evidenced && !state.evidenced.includes(submitted.sendId)) {
    if (!events.some((event) => event.kind === "chatItem" && event.item.id === id))
      events.push({ kind: "chatItem", threadId: state.sessionId, item: { ...state.items[id], content: submitted.input.text } });
    state.evidenced.push(submitted.sendId);
    events.push({ kind: "sendEvidence", threadId: state.sessionId, sendId: submitted.sendId, turnId: `${state.sessionId}:turn:${state.turn}`, itemId: id });
  }
  if (submitted.input.attachments.length) {
    for (const event of events) if (event.kind === "chatItem" && event.item.id === id)
      event.item.attachments = [...submitted.input.attachments];
  }
}
function reduceStream(ctx, raw, frames, cursor, submitted, retainSubmitted = false) {
  if (!ctx.threadId) return { state: raw, events: [] };
  const state = initial(ctx, raw, cursor);
  const events = [];
  const ownSubmitted = submitted && ctx.claimedSends.some((claim) => claim.sendId === submitted.sendId && claim.threadId === state.sessionId) ? submitted : void 0;
  if (retainSubmitted && ownSubmitted && !state.loading && !state.settled) submittedRow(state, events, ownSubmitted, false);
  let bounded = false;
  for (const frame of frames) {
    const params = object(frame.params);
    if (params.sessionId !== ctx.threadId) continue;
    try {
      if (ownSubmitted && !state.loading && !state.settled) submittedRow(state, events, ownSubmitted, false);
      if (frame.kind === "request" && clientRequest(frame.method)) {
        if (!cursor.unattributed) startTurn(ctx, state, events);
        continue;
      }
      if (frame.kind !== "notification" || frame.method !== "session/update") continue;
      const update = object(params.update);
      const kind = text(update.sessionUpdate);
      if (!kind) continue;
      if (!cursor.unattributed) startTurn(ctx, state, events);
      if (ownSubmitted && !state.loading && !state.settled) submittedRow(state, events, ownSubmitted, true);
      if (kind === "user_message_chunk" || kind === "agent_message_chunk" || kind === "agent_thought_chunk") {
        const content = object(update.content);
        if (content.type !== "text" || typeof content.text !== "string" || state.settled && !state.loading) continue;
        const chunkText = content.text;
        if (kind === "user_message_chunk" && submitted && state.evidenced.includes(submitted.sendId)) continue;
        const messageId = text(update.messageId);
        const explicit = messageId ? `${kind}:${messageId}` : void 0;
        const base = explicit ? `${state.sessionId}:turn:${state.turn}:message:${explicit}` : void 0;
        if (!state.chunk || state.chunk.kind !== kind || base && state.chunk.id !== (state.offsetById[base]?.row ?? base)) {
          closeChunk(ctx, state, events);
          if (kind === "user_message_chunk" && state.loading && state.ordinal > 0) {
            state.turn++;
            state.ordinal = 0;
            state.seenItems = [];
            clearEntries(state, "offsetById");
            delete state.evictedMessages;
          }
          state.ordinal++;
          const key2 = explicit ? `${state.sessionId}:turn:${state.turn}:message:${explicit}` : void 0;
          const known = key2 ? state.offsetById[key2] : void 0;
          const resumed = key2 !== void 0 && !known && evictedMayInclude(state.evictedMessages, key2);
          const id = !key2 ? `${ctx.threadId}:turn:${state.turn}:item:${state.ordinal}` : known?.row ?? (resumed ? `${key2}:continued:${state.seq + 1}` : key2);
          state.chunk = { kind, id, offset: known?.offset ?? 0, chars: known?.chars ?? 0, ...known?.echo ? { echo: known.echo } : {} };
        }
        const chunk = state.chunk;
        if (kind === "user_message_chunk" && !state.loading) {
          const claim = ctx.claimedSends.find((claim2) => claim2.threadId === ctx.threadId && !state.evidenced.includes(claim2.sendId) && claim2.input.attachments.length === 0 && (chunk.chars === 0 || claim2.sendId === chunk.echo) && claim2.input.text.slice(chunk.chars, chunk.chars + chunkText.length) === chunkText);
          if (claim) chunk.echo = claim.sendId;
          else delete chunk.echo;
        }
        chunk.chars += content.text.length;
        const type = kind === "user_message_chunk" ? "user" : kind === "agent_thought_chunk" ? "thinking" : "assistant";
        const previous = state.items[chunk.id];
        const row = { id: chunk.id, seq: previous?.seq ?? ++state.seq, type, content: "", ...type === "thinking" ? { collapsed: true } : {} };
        events.push(chunk.offset ? { kind: "chatItemAppend", threadId: state.sessionId, itemId: row.id, content: content.text, offset: chunk.offset } : { kind: "chatItem", threadId: state.sessionId, item: { ...row, content: content.text } });
        chunk.offset += utf8Length(content.text);
        putEntry(state, "items", row.id, row);
        const key = explicit ? `${state.sessionId}:turn:${state.turn}:message:${explicit}` : void 0;
        if (key) rememberOffset(state, key, { offset: chunk.offset, chars: chunk.chars, ...chunk.echo ? { echo: chunk.echo } : {}, ...chunk.id !== key ? { row: chunk.id } : {} });
        if (!cursor.unattributed && !state.loading && !state.started && kind === "agent_message_chunk") events.push({ kind: "turnState", threadId: ctx.threadId, turnId: ctx.activeTurnId ?? `${ctx.threadId}:turn:${state.turn}`, state: "working" });
        continue;
      }
      if (kind === "tool_call" || kind === "tool_call_update") {
        if (state.settled && !state.loading) continue;
        closeChunk(ctx, state, events);
        const id = text(update.toolCallId);
        const previous = id ? state.items[id] : void 0;
        if (id && !state.seenItems.includes(id)) {
          state.ordinal++;
          state.seenItems.push(id);
        }
        const row = reduceTool(update, previous, state.seq + 1);
        if (row && (previous || kind === "tool_call")) {
          if (kind === "tool_call_update") {
            if (update.title === void 0) {
              delete row.toolTitle;
              delete row.toolName;
            }
            if (update.kind === void 0) delete row.toolKind;
          }
          emit(state, events, row, kind === "tool_call_update");
        }
      } else if (kind === "plan") {
        closeChunk(ctx, state, events);
        const id = `${ctx.threadId}:plan`;
        if (!state.seenItems.includes(id)) {
          state.ordinal++;
          state.seenItems.push(id);
        }
        emit(state, events, planRow(ctx.threadId, update, state.items[id], state.seq + 1));
      } else if (["available_commands_update", "config_option_update", "session_info_update", "current_mode_update"].includes(kind)) {
        rememberControl(state, kind, update);
        if (kind === "available_commands_update" && Array.isArray(update.availableCommands))
          events.push({ kind: "controlsConfirmed", threadId: ctx.threadId, settings: { commands: commands(update.availableCommands) } });
        if (kind === "config_option_update" && Array.isArray(update.configOptions))
          events.push({ kind: "controlsConfirmed", threadId: ctx.threadId, settings: sessionSettings(update) });
        if (kind === "current_mode_update" && typeof update.currentModeId === "string")
          events.push({ kind: "controlsConfirmed", threadId: ctx.threadId, settings: { mode: update.currentModeId } });
        const compaction = object(object(update._meta).compaction);
        if (kind !== "session_info_update" || !Object.keys(compaction).length) continue;
        const id = text(compaction.id) ?? `${ctx.threadId}:compaction:${state.turn}`;
        if (!state.compacted.includes(id)) {
          state.compacted.push(id);
          events.push({ kind: "contextCompacted", threadId: ctx.threadId, itemId: id });
        }
      } else if (kind === "usage_update") {
        const mapped = contextUsage(ctx.threadId, update);
        if (mapped) events.push(mapped);
      } else if (kind === "compaction_update" || kind === "compaction_summary_chunk") {
        const id = text(update.id) ?? `${ctx.threadId}:compaction:${state.turn}`;
        if (!state.compacted.includes(id)) {
          state.compacted.push(id);
          events.push({ kind: "contextCompacted", threadId: ctx.threadId, itemId: id });
        }
      } else if (kind === "agent_notice") {
        if (typeof update.message === "string" && update.message.length)
          events.push({ kind: "notice", threadId: ctx.threadId, message: update.message.slice(0, 4096) });
      } else warn(state, events, kind);
    } finally {
      boundState(state);
      bounded = true;
    }
  }
  if (!bounded) boundState(state);
  return { state: json(state), events };
}
function boundState(state) {
  for (const key of ["evidenced", "compacted", "seenItems"]) state[key] = state[key].slice(-32);
  let bytes = utf8Length(JSON.stringify({ ...state, items: {}, controls: {}, warnings: {}, offsetById: {} }));
  for (const key of mapKeys) bytes += mapBytes(state, key) - 2;
  for (const key of ["items", "controls", "warnings"]) {
    for (const id of Object.keys(state[key])) {
      if (bytes <= 47e3) break;
      const value = state[key][id];
      if (id === state.chunk?.id || key === "items" && object(value).toolStatus === "running") continue;
      bytes += removeEntry(state, key, id);
    }
  }
  for (const key of ["evidenced", "compacted", "seenItems"]) {
    while (bytes > 47e3 && state[key].length) {
      const count2 = state[key].length;
      const removed = state[key].shift();
      bytes -= utf8Length(JSON.stringify(removed)) + (count2 > 1 ? 1 : 0);
    }
  }
}

// plugins/provider-acp/src/runtimeReduceBudget.ts
var omitted = (value) => value === void 0 || typeof value === "function" || typeof value === "symbol";
var MAX_BYTES = 4 * 1024 * 1024 - 1024;
var MAX_EVENTS = 1024;
var MAX_STATE_BYTES = 256 * 1024;
var marker = "\n[ACP row truncated: runtime reduction byte limit]";
var utf8 = (code) => code <= 127 ? 1 : code <= 2047 ? 2 : code <= 65535 ? 3 : 4;
var escaped = (code) => code < 32 ? code === 8 || code === 9 || code === 10 || code === 12 || code === 13 ? 2 : 6 : code === 34 || code === 92 ? 2 : code >= 55296 && code <= 57343 ? 6 : utf8(code);
function stringBytes(value) {
  let json2 = 2, raw = 0;
  for (let index = 0; index < value.length; ) {
    const code = value.codePointAt(index);
    json2 += escaped(code);
    raw += utf8(code);
    index += code > 65535 ? 2 : 1;
  }
  return { json: json2, raw };
}
function size(value) {
  if (omitted(value)) return 0;
  if (typeof value === "string") return stringBytes(value).json;
  if (typeof value === "number") return Number.isFinite(value) ? String(value).length : 4;
  if (value === null) return 4;
  if (typeof value === "boolean") return value ? 4 : 5;
  if (typeof value !== "object") return MAX_BYTES + 1;
  if (Array.isArray(value)) return 2 + Math.max(0, value.length - 1) + value.reduce((sum, entry) => sum + (omitted(entry) ? 4 : size(entry)), 0);
  const entries = Object.entries(value).filter(([, entry]) => !omitted(entry));
  return 2 + Math.max(0, entries.length - 1) + entries.reduce((sum, [key, entry]) => sum + stringBytes(key).json + 1 + size(entry), 0);
}
function upperBound(value) {
  if (omitted(value)) return 0;
  if (typeof value === "string") return value.length * 6 + 2;
  if (typeof value === "number") return 32;
  if (value === null || typeof value === "boolean") return 5;
  if (typeof value !== "object") return MAX_BYTES + 1;
  let total = 2;
  if (Array.isArray(value)) {
    for (const entry of value) {
      total += (omitted(entry) ? 4 : upperBound(entry)) + 1;
      if (total > MAX_BYTES) break;
    }
  } else {
    for (const [key, entry] of Object.entries(value)) {
      if (omitted(entry)) continue;
      total += key.length * 6 + 4 + upperBound(entry);
      if (total > MAX_BYTES) break;
    }
  }
  return total;
}
function body(event) {
  if (event.kind === "chatItemAppend") return { id: event.itemId, content: event.content };
  if (event.kind === "chatItem" && ["user", "assistant", "thinking"].includes(event.item.type)) return event.item;
}
function replace(event, content) {
  return event.kind === "chatItem" ? { ...event, item: { ...event.item, content } } : { ...event, content };
}
function prefix(value, budget) {
  let length = 0, cost = 0, raw = 0;
  while (length < value.length) {
    const code = value.codePointAt(length), next = escaped(code);
    if (next > budget) break;
    budget -= next;
    cost += next;
    raw += utf8(code);
    length += code > 65535 ? 2 : 1;
  }
  return { value: value.slice(0, length), cost, raw };
}
function shift(state, events, index, id, removed) {
  forgetAccounting(state);
  let delta = 0;
  for (let next = index + 1; next < events.length; next++) {
    const event = events[next];
    if (event.kind === "chatItemAppend" && event.itemId === id) {
      delta += String(event.offset - removed).length - String(event.offset).length;
      events[next] = { ...event, offset: event.offset - removed };
    }
  }
  const cursor = state;
  if (cursor.chunk?.id === id) {
    const before = size(cursor.chunk);
    cursor.chunk = { ...cursor.chunk, offset: cursor.chunk.offset - removed };
    delete cursor.chunk.echo;
    delta += size(cursor.chunk) - before;
  }
  const key = Object.keys(cursor.offsetById ?? {}).find((key2) => (cursor.offsetById[key2].row ?? key2) === id);
  const previous = key === void 0 ? void 0 : cursor.offsetById[key];
  if (key !== void 0 && previous) {
    cursor.offsetById[key] = { ...previous, offset: previous.offset - removed };
    delete cursor.offsetById[key].echo;
    delta += size(cursor.offsetById[key]) - size(previous);
  }
  return delta;
}
function fitTextReduction(state, source) {
  if (source.length <= MAX_EVENTS && upperBound(source) + MAX_STATE_BYTES + 32 <= MAX_BYTES) return { state, events: source };
  const events = [...source];
  const bounded = { state, events };
  if (events.length > MAX_EVENTS) {
    let thread;
    for (let index = events.length - 1; index >= 0 && events.length >= MAX_EVENTS; index--) {
      const text2 = body(events[index]);
      if (!text2) continue;
      const event = events[index];
      if ("threadId" in event) thread = event.threadId;
      shift(state, events, index, text2.id, stringBytes(text2.content).raw);
      events.splice(index, 1);
    }
    if (thread) events.push({ kind: "notice", threadId: thread, message: "ACP text omitted: runtime reduction event limit" });
  }
  let measured = size({ state, events: [] }) + Math.max(0, events.length - 1);
  const candidates = events.flatMap((event, index) => {
    const text2 = body(event);
    if (!text2) {
      measured += size(event);
      return [];
    }
    const { json: cost, raw } = stringBytes(text2.content);
    measured += size(replace(event, "")) - 2 + cost;
    return [{ index, text: text2, cost, raw }];
  });
  let excess = measured - MAX_BYTES;
  if (excess <= 0) return bounded;
  const markerBytes = stringBytes(marker);
  candidates.sort((a, b) => b.cost - a.cost || a.index - b.index);
  for (const { index, text: text2, cost, raw } of candidates) {
    if (excess <= 0) break;
    if (cost <= markerBytes.json) continue;
    const kept = prefix(text2.content, Math.max(0, cost - 2 - excess - (markerBytes.json - 2) - 64));
    events[index] = replace(events[index], kept.value + marker);
    excess -= cost - (kept.cost + markerBytes.json);
    excess += shift(state, events, index, text2.id, raw - kept.raw - markerBytes.raw);
  }
  return bounded;
}

// plugins/provider-acp/src/runtimePrompts.ts
var kinds = /* @__PURE__ */ new Set(["allow_once", "allow_always", "reject_once", "reject_always"]);
var kindLabels = { allow_once: "Allow once", allow_always: "Always allow", reject_once: "Reject", reject_always: "Always reject" };
var actions = { read: "read", search: "read", edit: "write", delete: "write", move: "write", execute: "command" };
var object3 = (value) => value !== null && typeof value === "object" && !Array.isArray(value) ? value : {};
function fail(message) {
  throw { code: "provider_refused", message };
}
function readable(value, limit = 384) {
  if (typeof value !== "string") return "";
  const text2 = value.slice(0, limit + 1).replace(/\r\n?/g, "\n").replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g, "");
  if (value.length <= limit) return text2;
  let end = Math.min(text2.length, limit - 1);
  if (/[\uD800-\uDBFF]/.test(text2[end - 1])) end--;
  return `${text2.slice(0, end)}\u2026`;
}
function lineCount(value) {
  if (typeof value !== "string" || !value.length) return 0;
  let count2 = value.endsWith("\n") ? 0 : 1;
  for (let i = 0; i < value.length; i++) if (value[i] === "\n") count2++;
  return count2;
}
function preview(value, prefix2 = "") {
  const text2 = readable(value);
  if (!text2) return [];
  const lines = text2.split("\n", 4);
  if (lines[lines.length - 1] === "") lines.pop();
  return lines.slice(0, 3).map((line) => `${prefix2}${line}`).concat(lines.length > 3 ? ["\u2026"] : []);
}
function displayPath(path) {
  if (path.startsWith("\\\\?\\UNC\\")) return "\\\\" + path.slice(8);
  return path.replace(/^(?:\\\\\?\\|\/\/\?\/)(?=[a-z]:[\\/])/i, "").replace(/[\x00-\x1f\x7f]/g, "\uFFFD");
}
function writeHeader(tool) {
  if (tool.kind !== "edit" || !Array.isArray(tool.content)) return "";
  const diffs = tool.content.map(object3).filter((entry) => entry.type === "diff");
  const path = diffs[0]?.path;
  if (diffs.length !== 1 || typeof path !== "string" || !path.length || path.length > 4096) return "";
  return `Writes ${displayPath(path)} (+${lineCount(diffs[0].newText)}/-${lineCount(diffs[0].oldText)} lines)
`;
}
function permissionQuestion(tool) {
  const title = readable(tool.title, 256).trim() || "Tool permission requested";
  const kind = readable(tool.kind, 64).trim();
  const lines = [kind && kind !== "other" ? `${title}
Kind: ${kind}` : title];
  const paths = /* @__PURE__ */ new Set();
  const addPath = (value) => {
    const path = readable(value, 512);
    if (path && !paths.has(path) && paths.size < 4) {
      paths.add(path);
      lines.push(`Path: ${path}`);
    }
  };
  if (Array.isArray(tool.locations)) for (const location of tool.locations.slice(0, 4)) addPath(object3(location).path);
  const input = object3(tool.rawInput);
  addPath(input.path);
  const command = readable(input.command || input.commandLine || input.cmd, 768);
  if (command) lines.push(`Command: ${command}`);
  if (Array.isArray(tool.content)) for (const entry of tool.content.slice(0, 4)) {
    const content = object3(entry);
    if (content.type === "diff") {
      addPath(content.path);
      lines.push(`Diff: ${readable(content.path, 512) || "file"} (+${lineCount(content.newText)} added, -${lineCount(content.oldText)} removed lines)`);
      lines.push(...preview(content.oldText, "- "), ...preview(content.newText, "+ "));
    } else if (content.type === "content") {
      const block = object3(content.content);
      if (block.type === "text") lines.push(...preview(block.text));
    }
  }
  lines.push("Allow this tool call?");
  const header = writeHeader(tool);
  const budget = Math.max(0, 2048 - header.length);
  return header + (budget ? readable(lines.join("\n"), budget) : "");
}
function runtimePrompt(ctx, method, params) {
  const data = object3(params);
  if (method !== "session/request_permission" || !ctx.threadId || data.sessionId !== ctx.threadId) return void 0;
  const tool = object3(data.toolCall);
  if (typeof tool.toolCallId !== "string" || !tool.toolCallId || !Array.isArray(data.options) || !data.options.length || data.options.length > 16) return void 0;
  const seen = /* @__PURE__ */ new Set();
  const options = data.options.map(object3);
  if (options.some((option) => {
    if (typeof option.optionId !== "string" || !option.optionId || option.optionId.length > 4096 || seen.has(option.optionId) || typeof option.name !== "string" || option.name.length > 4096 || typeof option.kind !== "string" || !kinds.has(option.kind)) return true;
    seen.add(option.optionId);
    return false;
  })) return void 0;
  const question = permissionQuestion(tool);
  return {
    prompt: { question, options: options.map((option) => ({ label: readable(option.name, 128).trim() || kindLabels[option.kind], value: String(option.optionId) })) },
    permission: { action: actions[String(tool.kind)] ?? "other", options: options.map((option) => ({ value: String(option.optionId), decision: option.kind })) }
  };
}
function runtimeAnswer(ctx, method, params, _id, answer) {
  const prompt = runtimePrompt(ctx, method, params)?.prompt;
  if (!prompt) fail("Permission request does not belong to this session");
  if (answer.kind === "cancel") return { outcome: { outcome: "cancelled" } };
  if (answer.kind !== "choice" || answer.values.length !== 1 || !prompt.options.some((option) => option.value === answer.values[0]))
    fail("Permission requires one declared optionId");
  return { outcome: { outcome: "selected", optionId: answer.values[0] } };
}

// plugins/provider-acp/src/launchSelection.ts
function launchSelection(requested, reported, attempted) {
  for (const [category, value, title] of [["model", requested.model, "model"], ["thought_level", requested.effort, "reasoning effort"]]) {
    if (!value) continue;
    const option = reported?.configOptions?.find((option2) => option2.category === category);
    const offered = option?.options.map((choice) => choice.value).join(", ") || "none";
    const choices = Array.from(offered).slice(0, 2e3).join("");
    if (!option || !option.options.some((choice) => choice.value === value))
      return { kind: "refused", message: `Requested ${title} '${value}' is not offered; offered values: ${choices}` };
    if (option.currentValue === value) continue;
    if (attempted.includes(category))
      return { kind: "refused", message: `Agent did not confirm requested ${title} '${value}'; offered values: ${choices}` };
    return { kind: "set", category, id: option.id, value };
  }
  return { kind: "complete" };
}

// plugins/provider-acp/src/runtime.ts
var epochs = /* @__PURE__ */ new Map();
var object4 = (value) => value !== null && typeof value === "object" && !Array.isArray(value) ? value : {};
var record = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
function fail2(code, message) {
  throw { code, message };
}
function refused(code, message) {
  return { kind: "refused", reason: { code, message }, beforeAcceptance: true };
}
function binding(ctx, op) {
  if (!ctx.tabId || !ctx.bindingNonce || !Number.isSafeInteger(ctx.epoch)) fail2("binding_mismatch", "Missing runtime binding");
  if (op && "threadId" in op && op.threadId && ctx.threadId !== op.threadId)
    fail2("binding_mismatch", "Session does not belong to this tab");
}
function negotiated(ctx) {
  const state = epochs.get(ctx.tabId);
  if (!state || state.epoch !== ctx.epoch || state.nonce !== ctx.bindingNonce) return void 0;
  touch(ctx.tabId, state);
  return state;
}
function refusePlan(reason) {
  return { action: { kind: "refused", reason }, state: null };
}
function touch(tabId, state) {
  epochs.delete(tabId);
  epochs.set(tabId, state);
}
function remember(ctx, state) {
  epochs.delete(ctx.tabId);
  if (epochs.size >= 256) {
    const oldest = epochs.keys().next();
    if (!oldest.done) epochs.delete(oldest.value);
  }
  epochs.set(ctx.tabId, state);
}
function request(method, params) {
  return { action: { kind: "request", call: { method, params } }, state: { method } };
}
function ownSession(ctx, state) {
  if (!ctx.threadId || state.sessionId !== ctx.threadId) fail2("binding_mismatch", "ACP session is not bound to this tab");
  return ctx.threadId;
}
function agentIdentity(raw) {
  const info = object4(raw);
  if (!label(info.name) || !label(info.version) || info.title !== void 0 && !label(info.title)) return {};
  return { agentName: typeof info.title === "string" ? info.title : info.name, binaryVersion: info.version };
}
function sessionAuthentication(state) {
  if (!state.authMethods.length || state.authenticated === true && state.required !== true) return [];
  state.authenticated = true;
  state.required = false;
  return [{ kind: "authentication", authentication: authentication(state.authMethods, true, false) }];
}
function threadNotices(state, threadId, events) {
  if (state.mcpUnavailable) {
    events.push({ kind: "notice", threadId, message: state.mcpUnavailable });
    delete state.mcpUnavailable;
  }
  return events.length ? { events } : {};
}
function applySettings(state, settings, configResponse = false) {
  if (configResponse && settings.configOptions) {
    const fallback = state.settings?.configOptions?.find((option) => option.id === state.modelStateId);
    if (modelOption(settings)) delete state.modelStateId;
    else if (fallback) {
      let id = fallback.id;
      while (settings.configOptions.some((option) => option.id === id)) id += "-";
      state.modelStateId = id;
      settings.configOptions = [...settings.configOptions.slice(0, 63), { ...fallback, id }];
      settings.model = fallback.currentValue;
    }
  }
  if (settings.configOptions) {
    delete state.settings?.model;
    delete state.settings?.effort;
    const model = modelOption(settings)?.currentValue, effort = effortOption(settings)?.currentValue;
    if (model) settings.model = model;
    if (effort) settings.effort = effort;
  }
  state.settings = { ...state.settings, ...settings };
}
function optionRequest(state, sessionId, id, value) {
  const plan = id === state.modelStateId ? request("session/set_model", { sessionId, modelId: value }) : request("session/set_config_option", { sessionId, configId: id, value });
  if (id === state.modelStateId) plan.state = { method: "session/set_model", modelId: value };
  return plan;
}
function fullAccessMode(ctx, state) {
  const mode = object4(ctx.providerSettings).fullAccessMode;
  return typeof mode === "string" && mode !== state.settings?.mode && state.settings?.modes?.some((entry) => entry.id === mode) ? mode : void 0;
}
function loadedThread(ctx, op, state, sessionId, attempted = [], confirmAuthentication = true) {
  const selection = launchSelection(op.settings, state.settings, attempted);
  if (selection.kind === "refused") return refuseThread(op, state, "provider_refused", selection.message);
  if (selection.kind === "set") {
    const plan = optionRequest(state, sessionId, selection.id, selection.value);
    plan.state = { ...object4(plan.state), attempted: [...attempted, selection.category], confirmAuthentication };
    return { kind: "continue", plan };
  }
  const mode = attempted.includes("mode") ? void 0 : fullAccessMode(ctx, state);
  if (mode) {
    const plan = request("session/set_mode", { sessionId, modeId: mode });
    plan.state = { method: "session/set_mode", modeId: mode, attempted: [...attempted, "mode"], confirmAuthentication };
    return { kind: "continue", plan };
  }
  return {
    kind: "complete",
    result: { kind: "threadLoaded", threadId: sessionId, settings: state.settings ?? {} },
    ...threadNotices(state, sessionId, confirmAuthentication ? sessionAuthentication(state) : [])
  };
}
function modeContinuation(ctx, op, state, raw, mode) {
  if (!state.sessionActive || !state.sessionId || op.kind === "resumeThread" && state.sessionId !== op.threadId)
    return refused("binding_mismatch", "ACP session is not active in this runtime");
  const attempted = object4(raw).attempted;
  if (!Array.isArray(attempted) || attempted.length > 3 || attempted[attempted.length - 1] !== "mode" || !attempted.every((value) => value === "model" || value === "thought_level" || value === "mode"))
    return refused("protocol_error", "Invalid session selection continuation");
  if (mode) applySettings(state, { mode });
  return loadedThread(ctx, op, state, state.sessionId, attempted, object4(raw).confirmAuthentication === true);
}
function refuseThread(op, state, code, message) {
  if (op.kind === "openThread") {
    delete state.sessionId;
    delete state.sessionActive;
    delete state.settings;
    delete state.modelStateId;
    delete state.instructions;
    state.cursor = { replay: 0, prompt: 0, loading: false };
  }
  return refused(code, message);
}
function clientCapabilities(ctx) {
  return { fs: { readTextFile: ctx.clientBridges?.fsRead === true, writeTextFile: ctx.clientBridges?.fsWrite === true }, terminal: ctx.clientBridges?.terminal === true };
}
function sessionRequest(ctx, method, sessionId) {
  const mcpServers = hostMcpServers(ctx);
  const plan = request(method, { ...sessionId ? { sessionId } : {}, cwd: ctx.cwd, mcpServers });
  plan.state = { method, mcpOffered: mcpServers.length > 0 };
  return plan;
}
function hostMcpServers(ctx) {
  const descriptor = ctx.hostMcp;
  return descriptor ? [{ name: descriptor.name, command: descriptor.command, args: [...descriptor.args], env: Object.entries(descriptor.env ?? {}).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([name, value]) => ({ name, value })) }] : [];
}
function runtimePlan(ctx, op) {
  binding(ctx, op);
  if (op.kind === "initialize") {
    epochs.delete(ctx.tabId);
    return request(methods.initialize, {
      protocolVersion,
      clientCapabilities: clientCapabilities(ctx),
      clientInfo: { name: "domios", title: "Domios", version: ctx.hostVersion }
    });
  }
  const state = negotiated(ctx);
  if (!state) return refusePlan("ACP initialization is required for this runtime epoch");
  if (op.kind === "control" && op.action === "auth") {
    return request(methods.initialize, { protocolVersion, clientCapabilities: clientCapabilities(ctx), clientInfo: { name: "domios", title: "Domios", version: ctx.hostVersion } });
  }
  if (op.kind === "control" && op.action === "login") {
    try {
      const method = agentAuthMethod(state.authMethods, object4(op.value).methodId);
      return request("authenticate", { methodId: method.id });
    } catch {
      return refusePlan("Authentication method was not declared or is unavailable");
    }
  }
  if (op.kind === "control" && op.action === "settings") {
    const sessionId = ownSession(ctx, state);
    const value = object4(op.value);
    const option = modelOption(state.settings);
    if (value.model === void 0) return refusePlan("Settings control requires a model change");
    if (value.model === option?.currentValue) return refusePlan("Settings already match the reported session model");
    if (!option) return refusePlan("This agent declared no model option");
    if (!option.options.some((entry) => entry.value === value.model)) return refusePlan("Model was not declared by this agent");
    return optionRequest(state, sessionId, option.id, value.model);
  }
  if (op.kind === "control" && ["config", "mode"].includes(op.action)) {
    const sessionId = ownSession(ctx, state);
    const value = object4(op.value);
    if (op.action === "mode") {
      if (typeof op.value !== "string" || !state.settings?.modes?.some((mode) => mode.id === op.value))
        return refusePlan("Session mode was not advertised");
      return request("session/set_mode", { sessionId, modeId: op.value });
    }
    const option = state.settings?.configOptions?.find((option2) => option2.id === value.id);
    if (!option?.options.some((option2) => option2.value === value.value)) return refusePlan("Configuration value was not advertised");
    return optionRequest(state, sessionId, option.id, value.value);
  }
  switch (op.kind) {
    case "answer": {
      ownSession(ctx, state);
      const pending = ctx.pendingRequest;
      if (!pending || pending.requestId !== op.requestId) return refusePlan("Unknown permission request");
      return { action: { kind: "reply", requestId: op.requestId, result: { kind: "success", value: runtimeAnswer(ctx, pending.method, pending.params, op.requestId, op.answer) } }, state: { method: pending.method } };
    }
    case "openThread":
      if (ctx.threadId || state.sessionId) return refusePlan("ACP session is already bound");
      return sessionRequest(ctx, methods.newSession);
    case "resumeThread": {
      if (state.sessionId && state.sessionId !== op.threadId) return refusePlan("ACP session does not belong to this runtime");
      if (state.sessionActive && state.sessionId === op.threadId)
        return { action: { kind: "local" }, state: { method: "boundSession" } };
      const capabilities = state.capabilities;
      const session = object4(capabilities.sessionCapabilities);
      const method = record(session.resume) ? methods.resume : capabilities.loadSession === true ? methods.load : null;
      if (!method) return refusePlan("ACP agent did not advertise session/load or session/resume");
      state.sessionActive = false;
      state.sessionId = op.threadId;
      state.cursor = { replay: state.cursor.replay + 1, prompt: 0, loading: true };
      return sessionRequest(ctx, method, op.threadId);
    }
    case "submitTurn": {
      const sessionId = ownSession(ctx, state);
      const slash = op.input.text.match(/^\/([^\s]+)(?:\s|$)/);
      if (slash && !state.settings?.commands?.some((command) => command.name === slash[1]))
        return refusePlan("Command was not advertised by this session");
      const images = [];
      for (const attachment of op.input.attachments) {
        if (!attachment.mimeType.startsWith("image/"))
          return refusePlan("This ACP agent cannot receive this attachment kind. Remove the attachment before sending.");
        if (object4(state.capabilities.promptCapabilities).image !== true)
          return refusePlan("This ACP agent did not declare image support. Remove image attachments before sending.");
        if (!attachment.data)
          return refusePlan("Image attachment bytes are unavailable. The message was not sent.");
        images.push({ type: "image", data: attachment.data, mimeType: attachment.mimeType });
      }
      const prompt = [];
      if (state.instructions) prompt.push({ type: "text", text: state.instructions });
      prompt.push({ type: "text", text: op.input.text });
      prompt.push(...images);
      if (state.inFlight && !ctx.activeTurnId) endAbandonedPrompts(ctx, state);
      const ordinal = Math.max(state.promptOrdinal ?? 0, state.cursor.prompt, Number(object4(state.projection).turn) || 0, ctx.restoredRowOrdinal ?? 0, ...(state.queued ?? []).map((value) => value.prompt)) + 1;
      state.promptOrdinal = ordinal;
      if (state.inFlight) {
        (state.queued ?? (state.queued = [])).push({ prompt: ordinal, sendId: op.sendId });
      } else {
        state.cursor.prompt = ordinal;
        state.cursor.loading = false;
        state.inFlight = true;
        state.promptSend = { sendId: op.sendId };
      }
      const plan = request(methods.prompt, { sessionId, prompt });
      if (plan.action.kind === "request")
        plan.action.call.target = { threadId: sessionId, turnId: `${sessionId}:turn:${ordinal}` };
      plan.state = { method: methods.prompt, prompt: ordinal };
      return plan;
    }
    case "interrupt": {
      const sessionId = ownSession(ctx, state);
      const turnId = op.activeTurnId ?? ctx.activeTurnId;
      if (turnId ? !ctx.activeTurns.some((turn) => turn.threadId === sessionId && turn.turnId === turnId) : !state.inFlight)
        return refusePlan("ACP cancel requires this tab's active prompt");
      return { action: { kind: "notification", notification: { method: methods.cancel, params: { sessionId }, target: { threadId: sessionId, ...turnId ? { turnId } : {} } } }, state: { method: methods.cancel } };
    }
    default:
      return refusePlan(`ACP operation is unavailable: ${op.kind}`);
  }
}
function endAbandonedPrompts(ctx, state) {
  state.inFlight = false;
  delete state.promptSend;
  delete state.queued;
  const projected = reduceStream(ctx, state.projection ?? null, [], state.cursor).state;
  state.projection = settleStream(ctx, projected, "cancelled").state;
}
function promoteQueued(ctx, state) {
  const sessionId = ownSession(ctx, state);
  const events = [];
  for (let queued = state.queued?.shift(); queued; queued = state.queued?.shift()) {
    state.cursor.prompt = queued.prompt;
    state.promptSend = { sendId: queued.sendId };
    state.inFlight = true;
    const next = reduceStream(ctx, state.projection ?? null, [], state.cursor);
    state.projection = { ...object4(next.state), started: true };
    events.push({ kind: "turnState", threadId: sessionId, turnId: `${sessionId}:turn:${queued.prompt}`, state: "working" });
    if (!queued.completed) break;
    const settledQueued = settleStream(ctx, state.projection, queued.completed.cancelled ? "cancelled" : "end_turn");
    state.projection = settledQueued.state;
    state.inFlight = false;
    if (queued.completed.usage) events.push({ kind: "usage", threadId: sessionId, usage: queued.completed.usage });
    events.push(...settledQueued.events);
  }
  return events;
}
function runtimeResponse(ctx, op, raw, reply) {
  binding(ctx, op);
  const method = object4(raw).method;
  if (reply.kind === "error") {
    const sessionOperation = op.kind === "openThread" && method === methods.newSession || op.kind === "resumeThread" && (method === methods.load || method === methods.resume);
    const state2 = negotiated(ctx);
    if (state2 && op.kind === "submitTurn" && method === methods.prompt) {
      const turn = object4(raw).prompt;
      state2.queued = state2.queued?.filter((queued) => queued.prompt !== turn || queued.sendId !== op.sendId);
      if (state2.cursor.prompt === turn && state2.promptSend?.sendId === op.sendId) {
        state2.inFlight = false;
        delete state2.promptSend;
        const projected = reduceStream(ctx, state2.projection ?? null, [], state2.cursor).state;
        state2.projection = settleStream(ctx, projected, "error").state;
        promoteQueued(ctx, state2);
      }
    }
    if (op.kind === "resumeThread" && sessionOperation && reply.error.code !== -32e3)
      return refused("session_load_refused", reply.error.message);
    if (sessionOperation && ![-32e3, -32002, -32601].includes(reply.error.code) && state2 && !state2.sessionActive && object4(raw).mcpOffered === true && ctx.hostMcp) {
      state2.mcpUnavailable = `Domios tools are unavailable for this agent: ${Array.from(reply.error.message).slice(0, 1e3).join("")}`;
      const plan = request(method, { ...op.kind === "resumeThread" ? { sessionId: op.threadId } : {}, cwd: ctx.cwd, mcpServers: [] });
      plan.state = { method, mcpOffered: false };
      return { kind: "continue", plan };
    }
    if (state2 && (op.kind === "openThread" || op.kind === "resumeThread") && method === "session/set_mode")
      return modeContinuation(ctx, op, state2, raw);
    if ((op.kind === "openThread" || op.kind === "resumeThread") && ["session/set_config_option", "session/set_model"].includes(String(method))) {
      const state3 = negotiated(ctx);
      if (!state3?.sessionActive || !state3.sessionId || op.kind === "resumeThread" && state3.sessionId !== op.threadId)
        return refused("binding_mismatch", "ACP session is not active in this runtime");
      return refuseThread(op, state3, "provider_refused", `Requested session options were refused: ${Array.from(reply.error.message).slice(0, 1e3).join("")}`);
    }
    if (reply.error.code === -32e3) {
      const state3 = negotiated(ctx);
      if (state3) {
        state3.authenticated = false;
        state3.required = true;
      }
      return refused("authentication_required", reply.error.message);
    }
    return refused("provider_refused", reply.error.message);
  }
  const value = object4(reply.value);
  if (op.kind === "control" && op.action === "login" && method === "authenticate") {
    const next = request(methods.initialize, { protocolVersion, clientCapabilities: clientCapabilities(ctx), clientInfo: { name: "domios", title: "Domios", version: ctx.hostVersion } });
    next.state = { method: methods.initialize, authenticated: true };
    return { kind: "continue", plan: next };
  }
  if (op.kind === "initialize" || op.kind === "control" && ["auth", "login"].includes(op.action)) {
    if (method !== methods.initialize || value.protocolVersion !== protocolVersion) {
      epochs.delete(ctx.tabId);
      return refused("protocol_error", "ACP agent must negotiate protocolVersion 1");
    }
    if (value.agentCapabilities !== void 0 && !record(value.agentCapabilities) || value.authMethods !== void 0 && !Array.isArray(value.authMethods))
      return refused("protocol_error", "Invalid ACP initialization response");
    let authMethods;
    try {
      authMethods = decodeAuthMethods(value.authMethods);
    } catch {
      return refused("protocol_error", "Invalid ACP authentication declaration");
    }
    const prior = negotiated(ctx);
    const authenticated = object4(raw).authenticated === true || prior?.authenticated === true;
    remember(ctx, {
      epoch: ctx.epoch,
      nonce: ctx.bindingNonce,
      capabilities: object4(value.agentCapabilities),
      authMethods,
      authenticated,
      required: authenticated ? false : prior?.required,
      sessionId: prior?.sessionId,
      sessionActive: prior?.sessionActive,
      instructions: prior?.instructions,
      cursor: prior?.cursor ?? { replay: 0, prompt: 0, loading: false },
      inFlight: prior?.inFlight ?? false,
      promptOrdinal: prior?.promptOrdinal,
      projection: prior?.projection,
      promptSend: prior?.promptSend,
      queued: prior?.queued,
      settings: prior?.settings,
      modelStateId: prior?.modelStateId
    });
    return {
      kind: "complete",
      result: op.kind === "initialize" ? { kind: "initialized", ...agentIdentity(value.agentInfo) } : { kind: "done" },
      events: [{ kind: "authentication", authentication: authentication(authMethods, authenticated, authenticated ? false : prior?.required) }]
    };
  }
  const state = negotiated(ctx);
  if (!state) return refused("provider_refused", "ACP initialization is required for this runtime epoch");
  if ((op.kind === "openThread" || op.kind === "resumeThread") && ["session/set_config_option", "session/set_model"].includes(String(method))) {
    if (!state.sessionActive || !state.sessionId || op.kind === "resumeThread" && state.sessionId !== op.threadId)
      return refused("binding_mismatch", "ACP session is not active in this runtime");
    const settings = method === "session/set_model" ? selectedSessionModel(state.settings, state.modelStateId, object4(raw).modelId) : Array.isArray(value.configOptions) ? sessionSettings(value) : void 0;
    if (!settings) return refuseThread(op, state, "protocol_error", "Invalid session option response");
    applySettings(state, settings, method === "session/set_config_option");
    const attempted = object4(raw).attempted;
    if (!Array.isArray(attempted) || attempted.length > 2 || !attempted.every((value2) => value2 === "model" || value2 === "thought_level"))
      return refused("protocol_error", "Invalid session selection continuation");
    return loadedThread(ctx, op, state, state.sessionId, attempted, object4(raw).confirmAuthentication === true);
  }
  if ((op.kind === "openThread" || op.kind === "resumeThread") && method === "session/set_mode") {
    if (typeof object4(raw).modeId !== "string") return refused("protocol_error", "Invalid session mode continuation");
    return modeContinuation(ctx, op, state, raw, object4(raw).modeId);
  }
  if (op.kind === "resumeThread" && method === "boundSession") {
    if (!state.sessionActive || state.sessionId !== op.threadId) return refused("binding_mismatch", "ACP session is not active in this runtime");
    return loadedThread(ctx, op, state, ownSession(ctx, state), [], false);
  }
  if (op.kind === "openThread" || op.kind === "resumeThread") {
    if (op.kind === "resumeThread") {
      const allowed = method === methods.load ? state.capabilities.loadSession === true : method === methods.resume && record(object4(state.capabilities.sessionCapabilities).resume);
      if (!allowed) return refused("provider_refused", "ACP session method was not advertised");
    } else if (method !== methods.newSession) return refused("protocol_error", "Invalid ACP session response");
    const sessionId = op.kind === "openThread" ? value.sessionId : op.threadId;
    if (typeof sessionId !== "string" || !sessionId || sessionId.length > 4096 || op.kind === "resumeThread" && value.sessionId !== void 0 && value.sessionId !== sessionId)
      return refused("binding_mismatch", "ACP session identity is invalid");
    state.sessionId = sessionId;
    state.sessionActive = true;
    state.instructions = op.instructions;
    state.cursor.loading = op.kind === "resumeThread";
    state.settings = { commands: [], configOptions: [], modes: [], ...sessionSettings(value) };
    state.modelStateId = sessionModel(value.models, configOptions(value.configOptions))?.id;
    return loadedThread(ctx, op, state, sessionId);
  }
  if (op.kind === "control" && ["config", "mode", "settings"].includes(op.action)) {
    const sessionId = ownSession(ctx, state);
    const settings = ["config", "settings"].includes(op.action) && method === "session/set_config_option" && Array.isArray(value.configOptions) ? sessionSettings(value) : ["config", "settings"].includes(op.action) && method === "session/set_model" ? selectedSessionModel(state.settings, state.modelStateId, object4(raw).modelId) ?? null : op.action === "mode" && method === "session/set_mode" && typeof op.value === "string" ? { mode: op.value } : null;
    if (!settings) return refused("protocol_error", "Invalid session control response");
    applySettings(state, settings, method === "session/set_config_option");
    return { kind: "complete", result: { kind: "done" }, events: [{ kind: "controlsConfirmed", threadId: sessionId, settings: op.action === "settings" ? state.settings ?? settings : settings }] };
  }
  if (op.kind === "answer" && method === "session/request_permission")
    return { kind: "complete", result: { kind: "done" }, events: [{ kind: "promptResolved", threadId: ownSession(ctx, state), requestId: op.requestId }] };
  if (op.kind === "interrupt" && method === methods.cancel) {
    return { kind: "complete", result: { kind: "done" }, events: [] };
  }
  if (op.kind === "submitTurn" && method === methods.prompt) {
    const sessionId = ownSession(ctx, state);
    if (typeof value.stopReason !== "string") return refused("protocol_error", "ACP prompt response is missing stopReason");
    const turn = typeof object4(raw).prompt === "number" ? object4(raw).prompt : object4(state.projection).turn;
    if (turn !== state.cursor.prompt || !state.inFlight) {
      const queued = state.queued?.find((queued2) => queued2.prompt === turn);
      if (queued && !queued.completed) queued.completed = { cancelled: value.stopReason === "cancelled", usage: usage(value.usage) };
      return { kind: "complete", result: { kind: "turnAccepted", threadId: sessionId, sendId: op.sendId, turnId: `${sessionId}:turn:${turn}` }, events: queued ? sessionAuthentication(state) : [] };
    }
    state.inFlight = false;
    state.instructions = void 0;
    const projected = reduceStream(ctx, state.projection ?? null, [], state.cursor, { sendId: op.sendId, input: op.input }, true);
    const settled = settleStream(ctx, projected.state, value.stopReason);
    state.projection = settled.state;
    const mapped = usage(value.usage);
    const events = [...sessionAuthentication(state), ...mapped ? [{ kind: "usage", threadId: sessionId, usage: mapped }] : [], ...projected.events, ...settled.events];
    events.push(...promoteQueued(ctx, state));
    return { kind: "complete", result: { kind: "turnAccepted", threadId: sessionId, sendId: op.sendId, turnId: `${sessionId}:turn:${turn}` }, events };
  }
  return refused("unsupported", "ACP response is unavailable");
}
function runtimeReduce(ctx, raw, frames) {
  binding(ctx);
  const state = negotiated(ctx);
  if (!ctx.threadId || !state || state.sessionId !== ctx.threadId) return { state: raw, events: [] };
  const own2 = frames.flatMap((frame, index) => object4(frame.params).sessionId === ctx.threadId && (frame.kind === "notification" && frame.method === methods.update || frame.kind === "request" && clientRequest(frame.method)) ? [{ frame, index }] : []);
  const prompts = frames.flatMap((frame) => {
    if (frame.kind !== "request") return [];
    const request2 = runtimePrompt(ctx, frame.method, frame.params);
    return request2 ? [{ kind: "prompt", threadId: ctx.threadId, requestId: frame.requestId, ...request2 }] : [];
  });
  if (!own2.length) return { state: raw, events: prompts };
  let projection = state.projection ?? raw;
  const events = [];
  for (const { frame, index } of own2) {
    const window = ctx.streamWindow;
    const witness = window?.turnRequests?.find((request2) => request2.sendId === state.promptSend?.sendId);
    const sequence = window?.frameSequences?.[index];
    const attributable = !window || witness?.written === true && witness.afterSequence !== void 0 && sequence !== void 0 && sequence > witness.afterSequence;
    const updateKind = object4(object4(frame.params).update).sessionUpdate;
    if ((ctx.restoredRowOrdinal ?? 0) > 0 && (state.cursor.loading || !attributable) && [
      "user_message_chunk",
      "agent_message_chunk",
      "agent_thought_chunk",
      "tool_call",
      "tool_call_update",
      "plan",
      "compaction_update",
      "compaction_summary_chunk"
    ].includes(String(updateKind))) continue;
    const cursor = attributable ? state.cursor : {
      ...state.cursor,
      prompt: Number(object4(projection).prompt) || 0,
      loading: state.cursor.loading || object4(projection).loading === true,
      unattributed: true
    };
    if (!attributable && projection === null)
      projection = { ...object4(reduceStream(ctx, null, [], cursor).state), turn: 0 };
    const frameContext = !window ? ctx : { ...ctx, claimedSends: ctx.claimedSends.filter((claim2) => window.turnRequests?.some((request2) => request2.sendId === claim2.sendId && request2.written === true && request2.afterSequence !== void 0 && sequence !== void 0 && sequence > request2.afterSequence)) };
    const claim = frameContext.claimedSends.find((claim2) => claim2.threadId === ctx.threadId && claim2.sendId === state.promptSend?.sendId);
    const reduced = reduceStream(frameContext, projection, [frame], cursor, state.inFlight && attributable && claim ? { sendId: claim.sendId, input: claim.input } : void 0);
    projection = reduced.state;
    events.push(...reduced.events);
  }
  const reduction = { state: projection, events };
  for (const event of reduction.events) if (event.kind === "controlsConfirmed") {
    applySettings(state, event.settings, true);
  }
  const bounded = fitTextReduction(projection, [...reduction.events, ...prompts]);
  state.projection = bounded.state;
  return bounded;
}

// plugins/provider-acp/src/registry.ts
var registryBounds = { bytes: 2 * 1024 * 1024, agents: 512, string: 4096, args: 128 };
var targets = /* @__PURE__ */ new Set(["darwin-aarch64", "darwin-x86_64", "linux-aarch64", "linux-x86_64", "windows-aarch64", "windows-x86_64"]);
function invalid() {
  throw new Error("Invalid or oversized ACP registry v1 index");
}
function object5(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalid();
  return value;
}
function fields(value, allowed) {
  if (Object.keys(value).some((key) => !allowed.includes(key))) invalid();
}
function string(value, limit = registryBounds.string) {
  if (typeof value !== "string" || !value.length || value.length > limit || /[\u0000-\u001f]/.test(value)) invalid();
  return value;
}
function args(value) {
  if (value === void 0) return [];
  if (!Array.isArray(value) || value.length > registryBounds.args) invalid();
  return value.map((value2) => value2 === "" ? "" : string(value2));
}
function env(value) {
  if (value === void 0) return {};
  const entries = Object.entries(object5(value));
  if (entries.length > 128) invalid();
  for (const [key, value2] of entries) {
    if (!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(key) || key.toUpperCase().startsWith("CODETERM")) invalid();
    if (value2 !== "") string(value2);
  }
  return Object.fromEntries(entries);
}
function url(value) {
  const text2 = string(value);
  if (!/^https:\/\/[^/@\s?#]+(?:\/[^\s#]*)?$/.test(text2)) invalid();
  return text2;
}
function distribution(value) {
  const raw = object5(value);
  fields(raw, ["binary", "npx", "uvx"]);
  if (!Object.keys(raw).length) invalid();
  const result = {};
  for (const kind of ["npx", "uvx"]) {
    if (raw[kind] === void 0) continue;
    const entry = object5(raw[kind]);
    fields(entry, ["package", "args", "env"]);
    result[kind] = { package: string(entry.package, 256), args: args(entry.args), env: env(entry.env) };
  }
  if (raw.binary !== void 0) {
    const entries = Object.entries(object5(raw.binary));
    if (!entries.length || entries.length > targets.size) invalid();
    result.binary = Object.fromEntries(entries.map(([target, value2]) => {
      if (!targets.has(target)) invalid();
      const entry = object5(value2);
      fields(entry, ["archive", "cmd", "args", "env", "sha256"]);
      const sha256 = entry.sha256 === void 0 ? void 0 : string(entry.sha256, 64);
      if (sha256 !== void 0 && !/^[a-f\d]{64}$/i.test(sha256)) invalid();
      return [target, { archive: url(entry.archive), cmd: string(entry.cmd), args: args(entry.args), env: env(entry.env), ...sha256 ? { sha256 } : {} }];
    }));
  }
  return result;
}
function utf8Bytes(value) {
  let size2 = 0;
  for (const character of value) {
    const point = character.codePointAt(0);
    size2 += point < 128 ? 1 : point < 2048 ? 2 : point < 65536 ? 3 : 4;
    if (size2 > registryBounds.bytes) invalid();
  }
  return size2;
}
function decodeRegistry(text2) {
  if (text2.length > registryBounds.bytes || utf8Bytes(text2) > registryBounds.bytes) invalid();
  const index = object5(JSON.parse(text2));
  fields(index, ["version", "agents"]);
  if (index.version !== "1.0.0" || !Array.isArray(index.agents) || index.agents.length > registryBounds.agents) invalid();
  const ids = /* @__PURE__ */ new Set();
  const agents = index.agents.map((value) => {
    const entry = object5(value);
    fields(entry, ["id", "name", "version", "description", "distribution", "repository", "website", "authors", "license", "license_url", "icon"]);
    const id = string(entry.id, 128);
    if (!/^[a-z][a-z0-9-]*$/.test(id) || ids.has(id)) invalid();
    ids.add(id);
    const version = string(entry.version, 128);
    if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(version)) invalid();
    for (const field of ["repository", "website", "license_url", "icon"]) if (entry[field] !== void 0) url(entry[field]);
    if (entry.license !== void 0) string(entry.license);
    if (entry.authors !== void 0 && (!Array.isArray(entry.authors) || entry.authors.length > 64)) invalid();
    if (Array.isArray(entry.authors)) entry.authors.forEach((author) => string(author));
    return { id, ...entry.icon === void 0 ? {} : { icon: url(entry.icon) }, name: string(entry.name, 256), version, description: string(entry.description), distribution: distribution(entry.distribution) };
  });
  return { version: "1.0.0", agents };
}
function searchRegistryCache(cache, query) {
  const text2 = cache.read();
  if (text2 === null) return [];
  const needle = query.trim().slice(0, 256).toLowerCase();
  return decodeRegistry(text2).agents.filter((agent) => `${agent.id} ${agent.name} ${agent.description}`.toLowerCase().includes(needle));
}
function saveRegistryCache(cache, text2) {
  decodeRegistry(text2);
  if (!cache.write(text2)) throw new Error("ACP registry cache could not be saved");
}

// plugins/provider-acp/src/discovery.ts
var indexUrl = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
function registryCache(host2) {
  const path = `${host2.dataDir()}/acp/cache/registry-v1.json`;
  return { read: () => host2.readFileHead(path, registryBounds.bytes + 1), write: (text2) => host2.writeFileAtomic(path, text2) };
}
function discoveryCommand(host2, ctx) {
  const cache = registryCache(host2);
  const result = () => ({ result: JSON.stringify(searchRegistryCache(cache, ctx.args.join(" "))) });
  try {
    if (ctx.verb === "registry-search") return result();
    if (ctx.verb === "registry-refresh") {
      return host2.fetch.async({ url: indexUrl, timeoutMs: 15e3 }, (response) => {
        if (response.status !== 200 || !response.body || response.error) return { error: "ACP registry unavailable; the previous cache is retained" };
        try {
          saveRegistryCache(cache, response.body);
          return result();
        } catch {
          return { error: "Invalid ACP registry; the previous cache is retained" };
        }
      });
    }
    return { error: "Unknown ACP command" };
  } catch {
    return { error: "ACP registry cache is unavailable or invalid" };
  }
}

// plugins/provider-acp/src/limits.ts
var decimal = /^-?\d{1,15}(?:\.\d{1,8})?$/;
var currency = /^[A-Z]{3}$/;
function deepseekBalance(body2) {
  const value = JSON.parse(body2);
  if (typeof value.is_available !== "boolean" || !Array.isArray(value.balance_infos) || value.balance_infos.length > 8) return null;
  const amounts = value.balance_infos.map((entry) => typeof entry?.currency === "string" && currency.test(entry.currency) && typeof entry.total_balance === "string" && decimal.test(entry.total_balance) ? { currency: entry.currency, amount: entry.total_balance } : null);
  return amounts.every(Boolean) ? { available: value.is_available, amounts } : null;
}
var readers = {
  "deepseek-balance": { url: "https://api.deepseek.com/user/balance", parse: deepseekBalance }
};
function snapshot(billingMode, details, nowMs) {
  return JSON.stringify({
    provider: "",
    account_id: null,
    captured_at_ms: nowMs,
    billing_mode: billingMode,
    limits_reported: true,
    session_pct: null,
    session_resets_at_ms: null,
    weekly_pct: null,
    weekly_resets_at_ms: null,
    spend_cents: null,
    spend_limit_cents: null,
    spend_remaining_cents: null,
    spend_resets_at_ms: null,
    raw_text: "",
    usage_details: { limits: [], dollar_credits: [], extra_usage: null, ...details }
  });
}
function readJson(fetch, opts, subject, parse) {
  return fetch(opts, (result) => {
    if (result.error) return { error: result.error };
    if (result.status !== 200) return { error: `${subject} endpoint answered HTTP ${result.status}` };
    if (!result.body || result.body.length > 64 * 1024) return { error: `${subject} response is empty or too large` };
    try {
      return parse(result.body);
    } catch {
      return { error: `${subject} response has an unexpected format` };
    }
  });
}
var CODE_ASSIST = "https://cloudcode-pa.googleapis.com/v1internal";
var GEMINI_QUOTA = "Gemini quota";
var modelId = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
function codeAssistRequest(method, body2) {
  return {
    url: `${CODE_ASSIST}:${method}`,
    method: "POST",
    timeoutMs: 15e3,
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body2),
    credential: { id: "geminiOauth", headers: { Authorization: "Bearer {accessToken}" } }
  };
}
function codeAssistLimits(body2) {
  const buckets = JSON.parse(body2).buckets;
  if (!Array.isArray(buckets) || buckets.length > 128) return null;
  const lowest = /* @__PURE__ */ new Map();
  for (const bucket of buckets) {
    if (bucket?.modelId == null || bucket.remainingFraction == null) continue;
    const fraction = bucket.remainingFraction;
    if (typeof bucket.modelId !== "string" || !modelId.test(bucket.modelId) || typeof fraction !== "number" || !(fraction >= 0 && fraction <= 1)) return null;
    const known = lowest.get(bucket.modelId);
    if (!known || fraction < known.fraction) lowest.set(bucket.modelId, { fraction, resetTime: bucket.resetTime });
  }
  return [...lowest].map(([label2, { fraction, resetTime }]) => {
    const resets = typeof resetTime === "string" ? Date.parse(resetTime) : NaN;
    return { label: label2, used_percent: Math.round((1 - fraction) * 1e3) / 10, resets_at: Number.isFinite(resets) ? resets : null };
  }).sort((a, b) => b.used_percent - a.used_percent || a.label.localeCompare(b.label));
}
function noGoogleQuotaReason(mode) {
  if (mode === "gemini-api-key") return "Gemini API-key quota has no public endpoint; check usage in Google AI Studio";
  if (mode === "vertex-ai") return "Vertex AI quota is not readable here; check usage in the Google Cloud console";
  if (mode === void 0) return "Gemini has no readable sign-in yet, so no quota can be read";
  return "This Gemini sign-in mode reports no quota";
}
function geminiCodeAssistQuota({ fetch, credentialPublic }, nowMs) {
  return credentialPublic("geminiSettings", (settings) => {
    if (settings?.selectedType !== "oauth-personal") return { error: noGoogleQuotaReason(settings?.selectedType) };
    return credentialPublic("geminiOauth", (login) => {
      if (!login) return { error: "Gemini's Google sign-in is not in a file Domios can read, so no quota can be read" };
      if (!(Number(login.expiresAt) > nowMs)) return { error: "Gemini's sign-in token has expired; quota reads again once Gemini renews it" };
      const metadata = { ideType: "IDE_UNSPECIFIED", platform: "PLATFORM_UNSPECIFIED", pluginType: "GEMINI" };
      return readJson(fetch, codeAssistRequest("loadCodeAssist", { metadata }), GEMINI_QUOTA, (body2) => {
        const project = JSON.parse(body2).cloudaicompanionProject;
        if (typeof project !== "string" || !project) return { error: "This Google account uses its own GOOGLE_CLOUD_PROJECT, which Domios cannot read, so no quota can be read" };
        return readJson(fetch, codeAssistRequest("retrieveUserQuota", { project }), GEMINI_QUOTA, (quotaBody) => {
          const limits = codeAssistLimits(quotaBody);
          if (!limits) return { error: `${GEMINI_QUOTA} response has an unexpected format` };
          if (!limits.length) return { error: "Gemini reported no model quota for this account" };
          return snapshot("subscription", { quota: { title: "Gemini model quota", limits } }, nowMs);
        });
      });
    });
  });
}
var quotaReaders = {
  "gemini-code-assist": geminiCodeAssistQuota
};
var own = (table, key) => Object.prototype.hasOwnProperty.call(table, key) ? table[key] : void 0;
function readProfileLimits(ports, request2, nowMs) {
  const source = request2.source;
  if (source.kind === "quota") {
    const reader2 = own(quotaReaders, source.reader);
    return reader2 ? reader2(ports, nowMs) : { error: "Profile names an unknown quota reader" };
  }
  if (source.kind !== "balance") return { error: "Profile declares no plugin-read limits source" };
  const reader = own(readers, source.reader);
  if (!reader) return { error: "Profile names an unknown balance reader" };
  return readJson(ports.fetch, {
    url: reader.url,
    method: "GET",
    timeoutMs: 15e3,
    headers: { Accept: "application/json" },
    profileSecret: { profile: request2.profile, env: source.credentialEnv, headers: { Authorization: "Bearer {secret}" } }
  }, "Balance", (body2) => {
    const balance = reader.parse(body2);
    return balance ? snapshot("api", { balance }, nowMs) : { error: "Balance response has an unexpected format" };
  });
}

// plugins/provider-acp/src/plugin.ts
function onAgentCommand(ctx) {
  return discoveryCommand(host, ctx);
}
var limitsPorts = {
  fetch: (opts, then) => host.fetch.async(opts, then),
  credentialPublic: (id, then) => host.io({ op: "credentialPublic", id }, (fields2) => then(fields2))
};
function readProfileLimits2(request2, nowMs) {
  return readProfileLimits(limitsPorts, request2, nowMs);
}
var plugin_default = { runtimePlan, runtimeResponse, runtimeReduce, onAgentCommand, readProfileLimits: readProfileLimits2 };
