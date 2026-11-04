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
var clientCapabilities = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false
};

// plugins/provider-acp/src/runtime.ts
var epochs = /* @__PURE__ */ new Map();
var object = (value) => value !== null && typeof value === "object" && !Array.isArray(value) ? value : {};
var record = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
function fail(code, message) {
  throw { code, message };
}
function refused(code, message) {
  return { kind: "refused", reason: { code, message }, beforeAcceptance: true };
}
function binding(ctx, op) {
  if (!ctx.tabId || !ctx.bindingNonce || !Number.isSafeInteger(ctx.epoch)) fail("binding_mismatch", "Missing runtime binding");
  if (op && "threadId" in op && op.threadId && ctx.threadId !== op.threadId)
    fail("binding_mismatch", "Session does not belong to this tab");
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
  if (!ctx.threadId || state.sessionId !== ctx.threadId) fail("binding_mismatch", "ACP session is not bound to this tab");
  return ctx.threadId;
}
function runtimePlan(ctx, op) {
  binding(ctx, op);
  if (op.kind === "initialize") {
    epochs.delete(ctx.tabId);
    return request(methods.initialize, {
      protocolVersion,
      clientCapabilities,
      clientInfo: { name: "domios", title: "Domios", version: ctx.hostVersion }
    });
  }
  const state = negotiated(ctx);
  if (!state) return refusePlan("ACP initialization is required for this runtime epoch");
  if (op.kind === "control" && op.action === "auth") {
    return request(methods.initialize, { protocolVersion, clientCapabilities, clientInfo: { name: "domios", title: "Domios", version: ctx.hostVersion } });
  }
  if (op.kind === "control" && op.action === "login") {
    try {
      const method = agentAuthMethod(state.authMethods, object(op.value).methodId);
      return request("authenticate", { methodId: method.id });
    } catch {
      return refusePlan("Authentication method was not declared or is unavailable");
    }
  }
  switch (op.kind) {
    case "openThread":
      if (ctx.threadId || state.sessionId) return refusePlan("ACP session is already bound");
      return request(methods.newSession, { cwd: ctx.cwd, mcpServers: [] });
    case "resumeThread": {
      const capabilities = state.capabilities;
      const session = object(capabilities.sessionCapabilities);
      const method = record(session.resume) ? methods.resume : capabilities.loadSession === true ? methods.load : null;
      if (!method) return refusePlan("ACP agent did not advertise session/load or session/resume");
      return request(method, { sessionId: op.threadId, cwd: ctx.cwd, mcpServers: [] });
    }
    case "submitTurn": {
      const sessionId = ownSession(ctx, state);
      if (op.input.attachments.length) return refusePlan("ACP lifecycle supports text prompts only");
      const prompt = [];
      if (state.instructions) prompt.push({ type: "text", text: state.instructions });
      prompt.push({ type: "text", text: op.input.text });
      return request(methods.prompt, { sessionId, prompt });
    }
    case "interrupt": {
      const sessionId = ownSession(ctx, state);
      const turnId = op.activeTurnId ?? ctx.activeTurnId;
      if (!turnId || !ctx.activeTurns.some((turn) => turn.threadId === sessionId && turn.turnId === turnId))
        return refusePlan("ACP cancel requires this tab's exact active turn");
      return { action: { kind: "notification", notification: { method: methods.cancel, params: { sessionId }, target: { threadId: sessionId, turnId } } }, state: { method: methods.cancel } };
    }
    default:
      return refusePlan(`ACP operation is unavailable: ${op.kind}`);
  }
}
function runtimeResponse(ctx, op, raw, reply) {
  binding(ctx, op);
  if (reply.kind === "error") return refused("provider_refused", reply.error.message);
  const value = object(reply.value);
  const method = object(raw).method;
  if (op.kind === "control" && op.action === "login" && method === "authenticate") {
    const next = request(methods.initialize, { protocolVersion, clientCapabilities, clientInfo: { name: "domios", title: "Domios", version: ctx.hostVersion } });
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
    const authenticated = object(raw).authenticated === true || prior?.authenticated === true;
    remember(ctx, {
      epoch: ctx.epoch,
      nonce: ctx.bindingNonce,
      capabilities: object(value.agentCapabilities),
      authMethods,
      authenticated,
      sessionId: prior?.sessionId,
      instructions: prior?.instructions
    });
    return op.kind === "initialize" ? { kind: "complete", result: { kind: "initialized" } } : { kind: "complete", result: { kind: "done" }, events: [{ kind: "auth", state: authenticated ? "signedIn" : "unknown" }] };
  }
  const state = negotiated(ctx);
  if (!state) return refused("provider_refused", "ACP initialization is required for this runtime epoch");
  if (op.kind === "openThread" || op.kind === "resumeThread") {
    if (op.kind === "resumeThread") {
      const allowed = method === methods.load ? state.capabilities.loadSession === true : method === methods.resume && record(object(state.capabilities.sessionCapabilities).resume);
      if (!allowed) return refused("provider_refused", "ACP session method was not advertised");
    } else if (method !== methods.newSession) return refused("protocol_error", "Invalid ACP session response");
    const sessionId = op.kind === "openThread" ? value.sessionId : op.threadId;
    if (typeof sessionId !== "string" || !sessionId || sessionId.length > 4096 || op.kind === "resumeThread" && value.sessionId !== void 0 && value.sessionId !== sessionId)
      return refused("binding_mismatch", "ACP session identity is invalid");
    state.sessionId = sessionId;
    state.instructions = op.instructions;
    return { kind: "complete", result: { kind: "threadLoaded", threadId: sessionId, settings: op.settings } };
  }
  if (op.kind === "interrupt" && method === methods.cancel) return { kind: "complete", result: { kind: "done" } };
  if (op.kind === "submitTurn" && method === methods.prompt) {
    const sessionId = ownSession(ctx, state);
    if (typeof value.stopReason !== "string") return refused("protocol_error", "ACP prompt response is missing stopReason");
    state.instructions = void 0;
    return {
      kind: "complete",
      result: { kind: "turnAccepted", threadId: sessionId, sendId: op.sendId },
      events: [{ kind: "turnState", threadId: sessionId, state: "idle" }]
    };
  }
  return refused("unsupported", "ACP response is unavailable");
}
function runtimeReduce(ctx, raw, frames) {
  binding(ctx);
  const events = [];
  const state = epochs.get(ctx.tabId);
  for (const frame of frames) {
    if (frame.kind !== "notification" || frame.method !== methods.update) continue;
    const params = object(frame.params);
    if (!ctx.threadId || params.sessionId !== ctx.threadId || !state || state.epoch !== ctx.epoch || state.nonce !== ctx.bindingNonce || state.sessionId !== ctx.threadId) continue;
    touch(ctx.tabId, state);
    const update = object(params.update);
    if (update.sessionUpdate === "agent_message_chunk") events.push({ kind: "turnState", threadId: ctx.threadId, state: "working" });
  }
  return { state: raw, events };
}

// plugins/provider-acp/src/registry.ts
var registryBounds = { bytes: 2 * 1024 * 1024, agents: 512, string: 4096, args: 128 };
var targets = /* @__PURE__ */ new Set(["darwin-aarch64", "darwin-x86_64", "linux-aarch64", "linux-x86_64", "windows-aarch64", "windows-x86_64"]);
function invalid() {
  throw new Error("Invalid or oversized ACP registry v1 index");
}
function object2(value) {
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
  const entries = Object.entries(object2(value));
  if (entries.length > 128) invalid();
  for (const [key, value2] of entries) {
    if (!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(key) || key.toUpperCase().startsWith("CODETERM")) invalid();
    if (value2 !== "") string(value2);
  }
  return Object.fromEntries(entries);
}
function url(value) {
  const text = string(value);
  if (!/^https:\/\/[^/@\s?#]+(?:\/[^\s#]*)?$/.test(text)) invalid();
  return text;
}
function distribution(value) {
  const raw = object2(value);
  fields(raw, ["binary", "npx", "uvx"]);
  if (!Object.keys(raw).length) invalid();
  const result = {};
  for (const kind of ["npx", "uvx"]) {
    if (raw[kind] === void 0) continue;
    const entry = object2(raw[kind]);
    fields(entry, ["package", "args", "env"]);
    result[kind] = { package: string(entry.package, 256), args: args(entry.args), env: env(entry.env) };
  }
  if (raw.binary !== void 0) {
    const entries = Object.entries(object2(raw.binary));
    if (!entries.length || entries.length > targets.size) invalid();
    result.binary = Object.fromEntries(entries.map(([target, value2]) => {
      if (!targets.has(target)) invalid();
      const entry = object2(value2);
      fields(entry, ["archive", "cmd", "args", "env", "sha256"]);
      const sha256 = entry.sha256 === void 0 ? void 0 : string(entry.sha256, 64);
      if (sha256 !== void 0 && !/^[a-f\d]{64}$/i.test(sha256)) invalid();
      return [target, { archive: url(entry.archive), cmd: string(entry.cmd), args: args(entry.args), env: env(entry.env), ...sha256 ? { sha256 } : {} }];
    }));
  }
  return result;
}
function utf8Bytes(value) {
  let size = 0;
  for (const character of value) {
    const point = character.codePointAt(0);
    size += point < 128 ? 1 : point < 2048 ? 2 : point < 65536 ? 3 : 4;
    if (size > registryBounds.bytes) invalid();
  }
  return size;
}
function decodeRegistry(text) {
  if (text.length > registryBounds.bytes || utf8Bytes(text) > registryBounds.bytes) invalid();
  const index = object2(JSON.parse(text));
  fields(index, ["version", "agents"]);
  if (index.version !== "1.0.0" || !Array.isArray(index.agents) || index.agents.length > registryBounds.agents) invalid();
  const ids = /* @__PURE__ */ new Set();
  const agents = index.agents.map((value) => {
    const entry = object2(value);
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
    return { id, name: string(entry.name, 256), version, description: string(entry.description), distribution: distribution(entry.distribution) };
  });
  return { version: "1.0.0", agents };
}
function searchRegistryCache(cache, query) {
  const text = cache.read();
  if (text === null) return [];
  const needle = query.trim().slice(0, 256).toLowerCase();
  return decodeRegistry(text).agents.filter((agent) => `${agent.id} ${agent.name} ${agent.description}`.toLowerCase().includes(needle));
}
function saveRegistryCache(cache, text) {
  decodeRegistry(text);
  if (!cache.write(text)) throw new Error("ACP registry cache could not be saved");
}

// plugins/provider-acp/src/discovery.ts
var indexUrl = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
function registryCache(host2) {
  const path = `${host2.dataDir()}/acp/cache/registry-v1.json`;
  return { read: () => host2.readFileHead(path, registryBounds.bytes + 1), write: (text) => host2.writeFileAtomic(path, text) };
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

// plugins/provider-acp/src/plugin.ts
function onAgentCommand(ctx) {
  return discoveryCommand(host, ctx);
}
var plugin_default = { runtimePlan, runtimeResponse, runtimeReduce, onAgentCommand };
