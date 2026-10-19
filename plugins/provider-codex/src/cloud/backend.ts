import type {RemoteEnvironment, RemoteTaskBackend, RemoteTaskBinding, RemoteTaskCapability, RemoteTaskDiscovery, RemoteTaskSnapshot} from "../../../../packages/plugin-sdk/src/remoteTasks";

type Row = Record<string, unknown>;
const row = (value: unknown): Row => value && typeof value === "object" && !Array.isArray(value) ? value as Row : {};
const rows = (value: unknown): Row[] => Array.isArray(value) ? value.map(row) : [];
const text = (value: unknown): string | null => typeof value === "string" ? value : null;

export interface CloudTaskTransport {
  request(method: "GET" | "POST", path: string, accountRef: string, body?: unknown): unknown;
  discoverRemoteTask(): RemoteTaskDiscovery;
}

export const CODEX_CLOUD_ACCOUNT_REF = "codexAuth";
export const CLOUD_SIGN_IN_REASON = "Sign in to Codex to use Cloud environments.";

export function cloudTaskPath(environmentId: string, taskId?: string): string {
  // Codex Settings IDs use the CodexApi task paths; legacy environment IDs keep the WHAM routes.
  const taskPath = environmentId.startsWith("asenvcfg_") ? "/api/codex/tasks" : "/wham/tasks";
  return taskId === undefined ? taskPath : `${taskPath}/${encodeURIComponent(taskId)}`;
}

export function cloudEnvironmentList(value: unknown): RemoteEnvironment[] {
  if (!Array.isArray(value)) throw new Error("Cloud environment response is invalid");
  return value.flatMap(entry => {
    const environment = row(entry);
    const id = text(environment.id);
    const label = text(environment.label) ?? id;
    return id && label ? [{id, label, accountRef: CODEX_CLOUD_ACCOUNT_REF}] : [];
  });
}

export function cloudEnvironmentConfigList(value: unknown): RemoteEnvironment[] {
  const response = row(value);
  const data = row(response.data);
  const collection = Array.isArray(value) ? value : [
    response.items,
    response.environmentConfigs,
    response.environment_configs,
    data.items,
    data.environmentConfigs,
    data.environment_configs,
  ].find(Array.isArray);
  if (!Array.isArray(collection)) throw new Error("Codex Cloud settings response is invalid");

  const environments = rows(collection).flatMap(environment => {
    const id = text(environment.id) ?? text(environment.environmentConfigId) ?? text(environment.environment_config_id);
    const label = text(environment.name)
      ?? text(environment.displayName)
      ?? text(environment.display_name)
      ?? text(environment.label)
      ?? id;
    return id && label ? [{id, label, accountRef: CODEX_CLOUD_ACCOUNT_REF}] : [];
  });
  if (collection.length > 0 && environments.length === 0) throw new Error("Codex Cloud settings response is invalid");
  return environments;
}

export function cloudTaskRequest(environmentId: string, branch: string, prompt: string) {
  return {
    new_task: {environment_id: environmentId, branch, run_environment_in_qa_mode: false},
    input_items: [{type: "message", role: "user", content: [{content_type: "text", text: prompt}]}],
  };
}

export function cloudTaskSnapshot(value: unknown, binding: RemoteTaskBinding): RemoteTaskSnapshot {
  const details = row(value);
  const task = row(details.task);
  const taskId = text(task.id);
  if (!taskId || taskId !== binding.snapshot?.taskId) throw new Error("Cloud task response does not match this tab");
  if (task.environment_id !== undefined && task.environment_id !== binding.environmentId) throw new Error("Cloud task environment does not match this tab");
  const assistant = row(details.current_assistant_turn);
  const error = row(assistant.error);
  const display = row(details.task_status_display ?? task.task_status_display);
  const turn = row(display.latest_turn_status_display);
  const status = text(turn.turn_status) ?? text(assistant.turn_status) ?? text(display.state);
  const output = rows(assistant.output_items);
  const result = output.filter(item => item.type === "message").flatMap(item =>
    Array.isArray(item.content) ? item.content.map(fragment => typeof fragment === "string" ? fragment : text(row(fragment).text) ?? "") : [],
  ).filter(Boolean).join("\n");
  const diffs = [...rows(row(details.current_diff_task_turn).output_items), ...output];
  const diff = diffs.map(item => text(row(item.output_diff).diff) ?? text(item.diff)).find(Boolean) ?? null;
  const failed = Object.keys(error).length > 0 || ["failed", "cancelled", "error"].includes(status ?? "");
  return {
    taskId, attempt: text(assistant.id),
    status: failed ? "error" : ["completed", "ready", "applied"].includes(status ?? "") ? "ready" : status === "in_progress" ? "running" : "pending",
    result, diff, cursor: null, error: failed ? "Remote task failed" : null,
  };
}

export function cloudTaskBackend(capability: RemoteTaskCapability, transport: CloudTaskTransport): RemoteTaskBackend {
  return {
    capability,
    environments: () => capability.environments,
    create(input) {
      const response = row(transport.request("POST", cloudTaskPath(input.environmentId), input.accountRef, cloudTaskRequest(input.environmentId, input.branch, input.prompt)));
      const taskId = text(row(response.task).id) ?? text(response.id);
      if (!taskId) throw new Error("Cloud did not acknowledge a task identity");
      return {taskId};
    },
    read(binding) {
      if (!binding.snapshot) throw new Error("Cloud task identity is missing");
      return cloudTaskSnapshot(transport.request("GET", cloudTaskPath(binding.environmentId, binding.snapshot.taskId), binding.accountRef), binding);
    },
  };
}
