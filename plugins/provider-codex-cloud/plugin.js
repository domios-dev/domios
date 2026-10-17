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

// plugins/provider-codex-cloud/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default
});
module.exports = __toCommonJS(plugin_exports);

// packages/plugin-sdk/src/remoteTaskStorage.ts
var RemoteTaskFileStorage = class {
  constructor(fs, root2) {
    this.fs = fs;
    this.root = root2;
  }
  path(tabId) {
    if (!/^[a-zA-Z0-9_-]+$/.test(tabId)) throw new Error("Invalid remote tab storage identity");
    return `${this.root}/${tabId}.json`;
  }
  read(tabId) {
    return this.readFile(this.path(tabId));
  }
  readFile(path) {
    const raw = this.fs.readFile(path);
    if (raw === null) return null;
    const binding = JSON.parse(raw);
    if (!binding || !Number.isSafeInteger(binding.revision) || binding.revision < 0 || !["creating", "create_uncertain", "bound"].includes(binding.phase) || ![binding.providerId, binding.backend, binding.environmentId, binding.accountRef, binding.branch].every((value) => typeof value === "string" && value.length > 0) || binding.phase === "bound" && (!binding.snapshot || typeof binding.snapshot.taskId !== "string" || !binding.snapshot.taskId)) {
      throw new Error("Stored remote task binding is invalid; creation was not repeated");
    }
    return binding;
  }
  compareExchange(tabId, revision, next) {
    const path = this.path(tabId);
    if (!this.fs.makeDirs(this.root)) throw new Error("Remote task storage is unavailable");
    const lock = this.fs.lockFile(`${path}.lock`);
    if (!lock) throw new Error("Remote task storage is busy; creation was not repeated");
    try {
      if ((this.readFile(path)?.revision ?? null) !== revision) return false;
      if (!this.fs.writeFileAtomic(path, JSON.stringify(next))) throw new Error("Remote task binding could not be saved");
      return true;
    } finally {
      this.fs.unlockFile(lock);
    }
  }
};

// plugins/provider-codex-cloud/src/backend.ts
var row = (value) => value && typeof value === "object" && !Array.isArray(value) ? value : {};
var rows = (value) => Array.isArray(value) ? value.map(row) : [];
var text = (value) => typeof value === "string" ? value : null;
function cloudTaskRequest(environmentId, branch, prompt) {
  return {
    new_task: { environment_id: environmentId, branch, run_environment_in_qa_mode: false },
    input_items: [{ type: "message", role: "user", content: [{ content_type: "text", text: prompt }] }]
  };
}
function cloudTaskSnapshot(value, binding) {
  const details = row(value);
  const task = row(details.task);
  const taskId = text(task.id);
  if (!taskId || taskId !== binding.snapshot?.taskId) throw new Error("Cloud task response does not match this tab");
  if (task.environment_id !== void 0 && task.environment_id !== binding.environmentId) throw new Error("Cloud task environment does not match this tab");
  const assistant = row(details.current_assistant_turn);
  const error = row(assistant.error);
  const display = row(details.task_status_display ?? task.task_status_display);
  const turn = row(display.latest_turn_status_display);
  const status = text(turn.turn_status) ?? text(assistant.turn_status) ?? text(display.state);
  const output = rows(assistant.output_items);
  const result = output.filter((item) => item.type === "message").flatMap(
    (item) => Array.isArray(item.content) ? item.content.map((fragment) => typeof fragment === "string" ? fragment : text(row(fragment).text) ?? "") : []
  ).filter(Boolean).join("\n");
  const diffs = [...rows(row(details.current_diff_task_turn).output_items), ...output];
  const diff = diffs.map((item) => text(row(item.output_diff).diff) ?? text(item.diff)).find(Boolean) ?? null;
  const failed = Object.keys(error).length > 0 || ["failed", "cancelled", "error"].includes(status ?? "");
  return {
    taskId,
    attempt: text(assistant.id),
    status: failed ? "error" : ["completed", "ready", "applied"].includes(status ?? "") ? "ready" : status === "in_progress" ? "running" : "pending",
    result,
    diff,
    cursor: null,
    error: failed ? text(error.message) ?? "Remote task failed" : null
  };
}
function cloudTaskBackend(capability2, transport) {
  return {
    capability: capability2,
    environments: () => capability2.environments,
    create(input) {
      const response = row(transport.request("POST", "/wham/tasks", input.accountRef, cloudTaskRequest(input.environmentId, input.branch, input.prompt)));
      const taskId = text(row(response.task).id) ?? text(response.id);
      if (!taskId) throw new Error("Cloud did not acknowledge a task identity");
      return { taskId };
    },
    read(binding) {
      if (!binding.snapshot) throw new Error("Cloud task identity is missing");
      return cloudTaskSnapshot(transport.request("GET", `/wham/tasks/${encodeURIComponent(binding.snapshot.taskId)}`, binding.accountRef), binding);
    }
  };
}

// packages/plugin-sdk/src/remoteTasks.ts
var RemoteTaskController = class {
  constructor(storage, backend2) {
    this.storage = storage;
    this.backend = backend2;
  }
  create(tabId, providerId, environment, branch, prompt) {
    if (this.backend.capability.availability.status !== "available") {
      throw new Error(this.backend.capability.availability.reason);
    }
    if (!branch.trim() || !prompt.trim()) throw new Error("A branch and task are required");
    const environments = this.backend.environments();
    if (!environments.some((row2) => row2.id === environment.id && row2.accountRef === environment.accountRef)) {
      throw new Error("Choose an environment available to this account");
    }
    const claim = {
      revision: 0,
      providerId,
      backend: this.backend.capability.backend,
      environmentId: environment.id,
      accountRef: environment.accountRef,
      branch,
      phase: "creating",
      snapshot: null
    };
    if (!this.storage.compareExchange(tabId, null, claim)) {
      throw new Error("This tab already has a remote task; creation was not repeated");
    }
    let acknowledgement;
    try {
      acknowledgement = this.backend.create({ environmentId: environment.id, accountRef: environment.accountRef, branch, prompt });
      if (!acknowledgement.taskId) throw new Error("Missing remote task acknowledgement");
    } catch {
      const uncertain = { ...claim, revision: 1, phase: "create_uncertain" };
      this.storage.compareExchange(tabId, claim.revision, uncertain);
      return uncertain;
    }
    const bound = {
      ...claim,
      revision: 1,
      phase: "bound",
      snapshot: { taskId: acknowledgement.taskId, attempt: null, status: "pending", result: "", diff: null, cursor: null, error: null }
    };
    if (!this.storage.compareExchange(tabId, claim.revision, bound)) {
      throw new Error("Remote creation was acknowledged but its binding could not be saved; do not submit again");
    }
    return bound;
  }
  restore(tabId) {
    const binding = this.storage.read(tabId);
    if (!binding) return null;
    if (binding.backend !== this.backend.capability.backend) throw new Error("Remote backend does not match the stored tab");
    if (binding.phase !== "bound" || !binding.snapshot) {
      if (binding.phase === "create_uncertain") return binding;
      const uncertain = { ...binding, revision: binding.revision + 1, phase: "create_uncertain" };
      if (!this.storage.compareExchange(tabId, binding.revision, uncertain)) return this.storage.read(tabId);
      return uncertain;
    }
    const snapshot = this.backend.read(binding);
    if (snapshot.taskId !== binding.snapshot.taskId) throw new Error("Remote response belongs to a different task");
    const updated = { ...binding, revision: binding.revision + 1, snapshot };
    if (!this.storage.compareExchange(tabId, binding.revision, updated)) return this.storage.read(tabId);
    return updated;
  }
};

// plugins/provider-codex-cloud/src/chatBackend.ts
function taskMessages(binding) {
  if (!binding) return [];
  if (binding.phase !== "bound" || !binding.snapshot) {
    return [{ id: "remote-create-uncertain", type: "assistant", content: "Remote task creation is unconfirmed. Check your Cloud task list before submitting another task. This tab will not resubmit it." }];
  }
  const task = binding.snapshot;
  const content = [
    `Remote task: ${task.status}`,
    task.error,
    task.result,
    task.diff ? `\`\`\`diff
${task.diff}
\`\`\`` : task.status === "ready" ? "No diff was produced." : null
  ].filter(Boolean).join("\n\n");
  return [{ id: `${task.taskId}:${binding.revision}`, type: "assistant", content }];
}
function remoteChatBackend(storage, backend2, providerId) {
  const controller = new RemoteTaskController(storage, backend2);
  const sessions = /* @__PURE__ */ new Map();
  return {
    openSession(ctx) {
      if (backend2.capability.availability.status === "unavailable") throw new Error(backend2.capability.availability.reason);
      const existing = controller.restore(ctx.tabId);
      if (!existing && !backend2.capability.environments.some((environment) => environment.id === ctx.preset)) {
        throw new Error("Choose a Cloud environment before opening a remote task");
      }
      sessions.set(ctx.tabId, ctx);
      if (!existing && ctx.systemPrompt?.trim()) {
        const environment = backend2.capability.environments.find((environment2) => environment2.id === ctx.preset);
        if (!environment) throw new Error("The selected environment is unavailable");
        try {
          controller.create(ctx.tabId, providerId, environment, "master", ctx.systemPrompt);
        } catch (error) {
          if (!storage.read(ctx.tabId)) throw error;
        }
      }
      return { sessionId: ctx.tabId };
    },
    sendMessage(sessionId, prompt) {
      const ctx = sessions.get(sessionId);
      if (!ctx) throw new Error("Remote task tab is not open");
      const environment = backend2.capability.environments.find((environment2) => environment2.id === ctx.preset);
      if (!environment) throw new Error("The selected environment is unavailable");
      controller.create(sessionId, providerId, environment, "master", prompt);
    },
    poll(sessionId, cursor) {
      if (!sessions.has(sessionId)) throw new Error("Remote task tab is not open");
      const binding = storage.read(sessionId);
      const messages = taskMessages(binding);
      const next = JSON.stringify(messages);
      return { messages: cursor === next ? [] : messages, cursor: next, done: binding?.snapshot?.status === "ready" || binding?.snapshot?.status === "error" };
    },
    closeSession(sessionId) {
      sessions.delete(sessionId);
    },
    listModels: () => [],
    listPresets: () => backend2.capability.environments.map((environment) => ({ id: environment.id, name: environment.label })),
    viewCall(method, args) {
      const input = args;
      const sessionId = input?.sessionId;
      if (!sessionId) throw new Error("Remote task tab is not open");
      if (method === "read") return storage.read(sessionId);
      if (method === "refresh") return controller.restore(sessionId);
      if (method === "submit") {
        if (!sessions.has(sessionId)) throw new Error("Remote task tab must reopen before submission");
        const environment = backend2.capability.environments.find((environment2) => environment2.id === sessions.get(sessionId)?.preset);
        if (!environment) throw new Error("The selected environment is unavailable");
        return controller.create(sessionId, providerId, environment, input?.branch ?? "master", input?.prompt ?? "");
      }
      throw new Error("This remote task operation is unsupported");
    }
  };
}

// plugins/provider-codex-cloud/src/plugin.ts
var manifest = JSON.parse(host.manifestJson() ?? "{}");
var capability = manifest.capabilities.chatBackend.remoteTask;
var root = host.fs.expandHome("~/.codeterm/codex-cloud/tasks");
if (!root) throw new Error("Remote task storage is unavailable");
var backend = cloudTaskBackend(capability, {
  request() {
    throw new Error("Live Cloud access has not been verified");
  }
});
var plugin = { ...remoteChatBackend(new RemoteTaskFileStorage(host.fs, root), backend, manifest.id) };
var plugin_default = plugin;
