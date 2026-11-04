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
  runtimePlan: () => runtimePlan,
  runtimeReduce: () => runtimeReduce,
  runtimeResponse: () => runtimeResponse
});
module.exports = __toCommonJS(plugin_exports);

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
    row.toolArgs = typeof update.rawInput === "string" ? update.rawInput : JSON.stringify(update.rawInput);
  }
  const statuses = { pending: "running", in_progress: "running", completed: "completed", failed: "failed", cancelled: "interrupted" };
  const status = Object.prototype.hasOwnProperty.call(statuses, String(update.status)) ? statuses[String(update.status)] : void 0;
  if (status) {
    row.toolStatus = status;
    row.toolError = status === "failed";
  }
  if (update.rawOutput !== void 0) row.toolResult = typeof update.rawOutput === "string" ? update.rawOutput : JSON.stringify(update.rawOutput);
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
function usage(value) {
  const raw = object(value);
  const count = (value2) => typeof value2 === "number" && Number.isSafeInteger(value2) && value2 >= 0;
  if (!count(raw.inputTokens) || !count(raw.outputTokens)) return void 0;
  const mapped = { input_tokens: raw.inputTokens, output_tokens: raw.outputTokens };
  if (count(raw.cachedReadTokens)) mapped.cache_read_tokens = raw.cachedReadTokens;
  if (count(raw.cachedWriteTokens)) mapped.cache_write_tokens = raw.cachedWriteTokens;
  if (count(raw.used)) mapped.used_tokens = raw.used;
  if (count(raw.size)) mapped.window_tokens = raw.size;
  return mapped;
}

// plugins/provider-acp/src/runtimeReduce.ts
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
      textById: /* @__PURE__ */ Object.create(null),
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
    textById: Object.assign(/* @__PURE__ */ Object.create(null), state.textById),
    warnings: Object.assign(/* @__PURE__ */ Object.create(null), state.warnings),
    controls: { ...state.controls },
    evidenced: [...state.evidenced],
    compacted: [...state.compacted],
    seenItems: [...state.seenItems],
    ...state.chunk ? { chunk: { ...state.chunk } } : {}
  };
  if (copy.replay !== cursor.replay) {
    copy.replay = cursor.replay;
    copy.turn = 1;
    copy.ordinal = 0;
    copy.seenItems = [];
    copy.textById = /* @__PURE__ */ Object.create(null);
    delete copy.chunk;
    copy.settled = false;
    copy.started = false;
  } else if (copy.prompt !== cursor.prompt) {
    copy.turn = Math.max(copy.turn + 1, cursor.prompt);
    copy.ordinal = 0;
    copy.seenItems = [];
    copy.textById = /* @__PURE__ */ Object.create(null);
    delete copy.chunk;
    copy.settled = false;
    copy.started = false;
  }
  copy.prompt = cursor.prompt;
  copy.loading = cursor.loading;
  return copy;
}
function emit(state, events, row) {
  const previous = state.items[row.id];
  row.seq = previous?.seq ?? ++state.seq;
  state.items[row.id] = row;
  if (JSON.stringify(previous) !== JSON.stringify(row)) events.push({ kind: "chatItem", threadId: state.sessionId, item: row });
}
function rememberControl(state, kind, update) {
  const encoded = JSON.stringify(update);
  if (encoded.length <= 16384) state.controls[kind] = JSON.parse(encoded);
}
function warn(state, events, rawKind) {
  const kind = rawKind.slice(0, 128);
  if (!Object.prototype.hasOwnProperty.call(state.warnings, kind) && Object.keys(state.warnings).length >= 32) return;
  const count = state.warnings[kind] ?? 0;
  state.warnings[kind] = Math.min(count + 1, Number.MAX_SAFE_INTEGER);
  if (!count) emit(state, events, {
    id: `${state.sessionId}:unknown:${state.epoch}:${kind}`,
    seq: 0,
    type: "system",
    content: `Ignored unknown ACP session update: ${kind}`,
    systemSubtype: "runtime_warning"
  });
}
function closeChunk(ctx, state, events) {
  const chunk = state.chunk;
  if (chunk?.kind === "user_message_chunk" && !state.loading) {
    const claim = ctx.claimedSends.find((claim2) => claim2.threadId === ctx.threadId && !state.evidenced.includes(claim2.sendId) && claim2.input.attachments.length === 0 && claim2.input.text === chunk.content);
    if (claim) {
      if (!events.some((event) => event.kind === "chatItem" && event.item.id === chunk.id))
        events.push({ kind: "chatItem", threadId: state.sessionId, item: state.items[chunk.id] });
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
  const next = { ...state, items: Object.assign(/* @__PURE__ */ Object.create(null), state.items), evidenced: [...state.evidenced], settled: true };
  const events = [];
  closeChunk(ctx, next, events);
  for (const row of Object.values(next.items)) if (row.toolStatus === "running")
    emit(next, events, { ...row, toolStatus: "interrupted", toolInterrupted: true });
  events.push({ kind: "turnState", threadId: ctx.threadId, state: "idle", ...reason === "cancelled" ? { detail: "interrupted" } : {} });
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
function reduceStream(ctx, raw, frames, cursor) {
  if (!ctx.threadId) return { state: raw, events: [] };
  const state = initial(ctx, raw, cursor);
  const events = [];
  for (const frame of frames) {
    const params = object(frame.params);
    if (params.sessionId !== ctx.threadId) continue;
    if (frame.kind === "request" && clientRequest(frame.method)) {
      startTurn(ctx, state, events);
      continue;
    }
    if (frame.kind !== "notification" || frame.method !== "session/update") continue;
    const update = object(params.update);
    const kind = text(update.sessionUpdate);
    if (!kind) continue;
    startTurn(ctx, state, events);
    if (kind === "user_message_chunk" || kind === "agent_message_chunk" || kind === "agent_thought_chunk") {
      const content = object(update.content);
      if (content.type !== "text" || typeof content.text !== "string" || state.settled && !state.loading) continue;
      const explicit = text(update.messageId);
      if (!state.chunk || state.chunk.kind !== kind || explicit && state.chunk.id !== explicit) {
        closeChunk(ctx, state, events);
        if (kind === "user_message_chunk" && state.loading && state.ordinal > 0) {
          state.turn++;
          state.ordinal = 0;
          state.seenItems = [];
        }
        state.ordinal++;
        const id = explicit ?? `${ctx.threadId}:turn:${state.turn}:item:${state.ordinal}`;
        state.chunk = { kind, id, content: explicit ? state.textById[id] ?? "" : "" };
      }
      state.chunk.content += content.text;
      if (explicit) state.textById[state.chunk.id] = state.chunk.content;
      const type = kind === "user_message_chunk" ? "user" : kind === "agent_thought_chunk" ? "thinking" : "assistant";
      emit(state, events, { id: state.chunk.id, seq: 0, type, content: state.chunk.content, ...type === "thinking" ? { collapsed: true } : {} });
      if (!state.loading && !state.started && kind === "agent_message_chunk") events.push({ kind: "turnState", threadId: ctx.threadId, turnId: ctx.activeTurnId ?? `${ctx.threadId}:turn:${state.turn}`, state: "working" });
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
      if (row) emit(state, events, row);
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
      const mapped = usage(update);
      if (mapped) events.push({ kind: "usage", threadId: ctx.threadId, usage: mapped });
    } else if (kind === "compaction_update" || kind === "compaction_summary_chunk") {
      const id = text(update.id) ?? `${ctx.threadId}:compaction:${state.turn}`;
      if (!state.compacted.includes(id)) {
        state.compacted.push(id);
        events.push({ kind: "contextCompacted", threadId: ctx.threadId, itemId: id });
      }
    } else if (kind === "agent_notice") rememberControl(state, kind, update);
    else warn(state, events, kind);
  }
  return { state: json(state), events };
}

// plugins/provider-acp/src/runtimePrompts.ts
var kinds = /* @__PURE__ */ new Set(["allow_once", "allow_always", "reject_once", "reject_always"]);
var object2 = (value) => value !== null && typeof value === "object" && !Array.isArray(value) ? value : {};
function fail(message) {
  throw { code: "provider_refused", message };
}
function runtimePrompt(ctx, method, params) {
  const data = object2(params);
  if (method !== "session/request_permission" || !ctx.threadId || data.sessionId !== ctx.threadId) return void 0;
  const tool = object2(data.toolCall);
  if (typeof tool.toolCallId !== "string" || !tool.toolCallId || !Array.isArray(data.options) || !data.options.length || data.options.length > 16) return void 0;
  const seen = /* @__PURE__ */ new Set();
  const options = data.options.map(object2);
  if (options.some((option) => {
    if (typeof option.optionId !== "string" || !option.optionId || option.optionId.length > 4096 || seen.has(option.optionId) || typeof option.name !== "string" || option.name.length > 4096 || typeof option.kind !== "string" || !kinds.has(option.kind)) return true;
    seen.add(option.optionId);
    return false;
  })) return void 0;
  const question = JSON.stringify({ toolCall: tool });
  if (question.length > 65536) return void 0;
  return { question, options: options.map((option) => ({ label: `${option.name} (${option.kind})`, value: String(option.optionId) })) };
}
function runtimeAnswer(ctx, method, params, _id, answer) {
  const prompt = runtimePrompt(ctx, method, params);
  if (!prompt) fail("Permission request does not belong to this session");
  if (answer.kind === "cancel") return { outcome: { outcome: "cancelled" } };
  if (answer.kind !== "choice" || answer.values.length !== 1 || !prompt.options.some((option) => option.value === answer.values[0]))
    fail("Permission requires one declared optionId");
  return { outcome: { outcome: "selected", optionId: answer.values[0] } };
}

// plugins/provider-acp/src/runtime.ts
var epochs = /* @__PURE__ */ new Map();
var object3 = (value) => value !== null && typeof value === "object" && !Array.isArray(value) ? value : {};
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
function runtimePlan(ctx, op) {
  binding(ctx, op);
  if (op.kind === "initialize") {
    epochs.delete(ctx.tabId);
    return request(methods.initialize, {
      protocolVersion,
      clientCapabilities: { fs: { readTextFile: ctx.clientBridges?.fsRead === true, writeTextFile: ctx.clientBridges?.fsWrite === true }, terminal: ctx.clientBridges?.terminal === true },
      clientInfo: { name: "domios", title: "Domios", version: ctx.hostVersion }
    });
  }
  const state = negotiated(ctx);
  if (!state) return refusePlan("ACP initialization is required for this runtime epoch");
  switch (op.kind) {
    case "answer": {
      ownSession(ctx, state);
      const pending = ctx.pendingRequest;
      if (!pending || pending.requestId !== op.requestId) return refusePlan("Unknown permission request");
      return { action: { kind: "reply", requestId: op.requestId, result: { kind: "success", value: runtimeAnswer(ctx, pending.method, pending.params, op.requestId, op.answer) } }, state: { method: pending.method } };
    }
    case "openThread":
      if (ctx.threadId || state.sessionId) return refusePlan("ACP session is already bound");
      return request(methods.newSession, { cwd: ctx.cwd, mcpServers: [] });
    case "resumeThread": {
      const capabilities = state.capabilities;
      const session = object3(capabilities.sessionCapabilities);
      const method = record(session.resume) ? methods.resume : capabilities.loadSession === true ? methods.load : null;
      if (!method) return refusePlan("ACP agent did not advertise session/load or session/resume");
      state.sessionId = op.threadId;
      state.cursor = { replay: state.cursor.replay + 1, prompt: 0, loading: true };
      return request(method, { sessionId: op.threadId, cwd: ctx.cwd, mcpServers: [] });
    }
    case "submitTurn": {
      const sessionId = ownSession(ctx, state);
      if (op.input.attachments.length) return refusePlan("ACP lifecycle supports text prompts only");
      const prompt = [];
      if (state.instructions) prompt.push({ type: "text", text: state.instructions });
      prompt.push({ type: "text", text: op.input.text });
      state.cursor.prompt = Math.max(state.cursor.prompt, Number(object3(state.projection).turn) || 0) + 1;
      state.cursor.loading = false;
      state.inFlight = true;
      return request(methods.prompt, { sessionId, prompt });
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
function runtimeResponse(ctx, op, raw, reply) {
  binding(ctx, op);
  if (reply.kind === "error") return refused("provider_refused", reply.error.message);
  const value = object3(reply.value);
  const method = object3(raw).method;
  if (op.kind === "initialize") {
    if (method !== methods.initialize || value.protocolVersion !== protocolVersion) {
      epochs.delete(ctx.tabId);
      return refused("protocol_error", "ACP agent must negotiate protocolVersion 1");
    }
    if (value.agentCapabilities !== void 0 && !record(value.agentCapabilities) || value.authMethods !== void 0 && !Array.isArray(value.authMethods))
      return refused("protocol_error", "Invalid ACP initialization response");
    remember(ctx, {
      epoch: ctx.epoch,
      nonce: ctx.bindingNonce,
      capabilities: object3(value.agentCapabilities),
      authMethods: Array.isArray(value.authMethods) ? value.authMethods : [],
      cursor: { replay: 0, prompt: 0, loading: false },
      inFlight: false
    });
    return { kind: "complete", result: { kind: "initialized" } };
  }
  const state = negotiated(ctx);
  if (!state) return refused("provider_refused", "ACP initialization is required for this runtime epoch");
  if (op.kind === "openThread" || op.kind === "resumeThread") {
    if (op.kind === "resumeThread") {
      const allowed = method === methods.load ? state.capabilities.loadSession === true : method === methods.resume && record(object3(state.capabilities.sessionCapabilities).resume);
      if (!allowed) return refused("provider_refused", "ACP session method was not advertised");
    } else if (method !== methods.newSession) return refused("protocol_error", "Invalid ACP session response");
    const sessionId = op.kind === "openThread" ? value.sessionId : op.threadId;
    if (typeof sessionId !== "string" || !sessionId || sessionId.length > 4096 || op.kind === "resumeThread" && value.sessionId !== void 0 && value.sessionId !== sessionId)
      return refused("binding_mismatch", "ACP session identity is invalid");
    state.sessionId = sessionId;
    state.instructions = op.instructions;
    state.cursor.loading = false;
    return { kind: "complete", result: { kind: "threadLoaded", threadId: sessionId, settings: op.settings } };
  }
  if (op.kind === "answer" && method === "session/request_permission")
    return { kind: "complete", result: { kind: "done" }, events: [{ kind: "promptResolved", threadId: ownSession(ctx, state), requestId: op.requestId }] };
  if (op.kind === "interrupt" && method === methods.cancel) {
    state.inFlight = false;
    const settled = settleStream(ctx, state.projection ?? null, "cancelled");
    state.projection = settled.state;
    state.settledProjection = settled.state;
    return { kind: "complete", result: { kind: "done" }, events: settled.events };
  }
  if (op.kind === "submitTurn" && method === methods.prompt) {
    state.inFlight = false;
    const sessionId = ownSession(ctx, state);
    if (typeof value.stopReason !== "string") return refused("protocol_error", "ACP prompt response is missing stopReason");
    state.instructions = void 0;
    const settled = settleStream(ctx, state.projection ?? null, value.stopReason);
    state.projection = settled.state;
    state.settledProjection = settled.state;
    const mapped = usage(value.usage);
    return {
      kind: "complete",
      result: { kind: "turnAccepted", threadId: sessionId, sendId: op.sendId },
      events: [...mapped ? [{ kind: "usage", threadId: sessionId, usage: mapped }] : [], ...settled.events]
    };
  }
  return refused("unsupported", "ACP response is unavailable");
}
function runtimeReduce(ctx, raw, frames) {
  binding(ctx);
  const state = negotiated(ctx);
  if (!ctx.threadId || !state || state.sessionId !== ctx.threadId) return { state: raw, events: [] };
  const own = frames.filter((frame) => object3(frame.params).sessionId === ctx.threadId && (frame.kind === "notification" && frame.method === methods.update || frame.kind === "request" && clientRequest(frame.method)));
  const prompts = frames.flatMap((frame) => {
    if (frame.kind !== "request") return [];
    const prompt = runtimePrompt(ctx, frame.method, frame.params);
    return prompt ? [{ kind: "prompt", threadId: ctx.threadId, requestId: frame.requestId, prompt }] : [];
  });
  if (!own.length) return { state: raw, events: prompts };
  const reduction = reduceStream(ctx, state.settledProjection ?? raw, own, state.cursor);
  delete state.settledProjection;
  state.projection = reduction.state;
  return { ...reduction, events: [...reduction.events, ...prompts] };
}

// plugins/provider-acp/src/plugin.ts
var plugin_default = { runtimePlan, runtimeResponse, runtimeReduce };
