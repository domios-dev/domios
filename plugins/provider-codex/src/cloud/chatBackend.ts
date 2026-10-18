import type {ChatBackend, ChatBackendOpenSessionCtx, NormalizedChatMessage, ViewCall} from "../../../../packages/plugin-sdk/src/capabilities";
import {RemoteTaskController, type RemoteTaskBackend, type RemoteTaskBinding, type RemoteTaskStorage} from "../../../../packages/plugin-sdk/src/remoteTasks";

function taskMessages(binding: RemoteTaskBinding | null): NormalizedChatMessage[] {
  if (!binding) return [];
  if (binding.phase !== "bound" || !binding.snapshot) {
    return [{id: "remote-create-uncertain", type: "assistant", content: "Remote task creation is unconfirmed. Check your Cloud task list before submitting another task. This tab will not resubmit it."}];
  }
  const task = binding.snapshot;
  const content = [
    `Remote task: ${task.status}`,
    task.error, task.result,
    task.diff ? `\`\`\`diff\n${task.diff}\n\`\`\`` : task.status === "ready" ? "No diff was produced." : null,
  ].filter(Boolean).join("\n\n");
  return [{id: `${task.taskId}:${binding.revision}`, type: "assistant", content}];
}

export function remoteChatBackend(storage: RemoteTaskStorage, backend: RemoteTaskBackend, providerId: string): ChatBackend & {viewCall: ViewCall} {
  const controller = new RemoteTaskController(storage, backend);
  const sessions = new Map<string, ChatBackendOpenSessionCtx>();
  return {
    openSession(ctx) {
      if ((ctx.config as {executionMode?: string} | null)?.executionMode !== backend.capability.mode) throw new Error("Choose the declared Cloud execution mode");
      if (backend.capability.availability.status === "unavailable") throw new Error(backend.capability.availability.reason);
      const saved = storage.read(ctx.tabId);
      if (saved && saved.providerId !== providerId) throw new Error("Stored task belongs to another provider");
      const existing = controller.restore(ctx.tabId);
      if (!existing && !backend.capability.environments.some(environment => environment.id === ctx.preset)) {
        throw new Error("Choose a Cloud environment before opening a remote task");
      }
      sessions.set(ctx.tabId, ctx);
      if (!existing && ctx.systemPrompt?.trim()) {
        const environment = backend.capability.environments.find(environment => environment.id === ctx.preset);
        if (!environment) throw new Error("The selected environment is unavailable");
        try {
          controller.create(ctx.tabId, providerId, environment, "master", ctx.systemPrompt);
        } catch (error) {
          if (!storage.read(ctx.tabId)) throw error;
        }
      }
      return {sessionId: ctx.tabId};
    },
    sendMessage(sessionId, prompt) {
      const ctx = sessions.get(sessionId);
      if (!ctx) throw new Error("Remote task tab is not open");
      const environment = backend.capability.environments.find(environment => environment.id === ctx.preset);
      if (!environment) throw new Error("The selected environment is unavailable");
      controller.create(sessionId, providerId, environment, "master", prompt);
    },
    poll(sessionId, cursor) {
      if (!sessions.has(sessionId)) throw new Error("Remote task tab is not open");
      const binding = storage.read(sessionId);
      const messages = taskMessages(binding);
      const next = JSON.stringify(messages);
      return {messages: cursor === next ? [] : messages, cursor: next, done: binding?.snapshot?.status === "ready" || binding?.snapshot?.status === "error"};
    },
    closeSession(sessionId) { sessions.delete(sessionId); },
    listModels: () => [],
    listPresets: () => backend.capability.environments.map(environment => ({id: environment.id, name: environment.label})),
    viewCall(method, args) {
      const input = args as {sessionId?: string; prompt?: string; branch?: string} | null;
      const sessionId = input?.sessionId;
      if (!sessionId) throw new Error("Remote task tab is not open");
      if (method === "read") {
        const binding = storage.read(sessionId);
        if (binding && (binding.executionMode !== backend.capability.mode || binding.providerId !== providerId)) throw new Error("Stored execution mode does not match Cloud");
        return binding;
      }
      if (method === "refresh") return controller.restore(sessionId);
      if (method === "submit") {
        if (!sessions.has(sessionId)) throw new Error("Remote task tab must reopen before submission");
        const environment = backend.capability.environments.find(environment => environment.id === sessions.get(sessionId)?.preset);
        if (!environment) throw new Error("The selected environment is unavailable");
        return controller.create(sessionId, providerId, environment, input?.branch ?? "master", input?.prompt ?? "");
      }
      throw new Error("This remote task operation is unsupported");
    },
  };
}
