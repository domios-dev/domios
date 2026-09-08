interface ActivityItem {
  type?: unknown;
  id?: unknown;
  command?: unknown;
  status?: unknown;
  exit_code?: unknown;
  aggregated_output?: unknown;
  stdout?: unknown;
  stderr?: unknown;
  kind?: unknown;
  agent_path?: unknown;
  agent_thread_id?: unknown;
}

export interface ActivityMessage {
  role: "assistant";
  uuid: string;
  blocks: { kind: string; data: Record<string, unknown> }[];
  ts: number | null;
  timestamp: string | null;
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

export function completedActivity(value: unknown, ts: number | null, timestamp: string | null): ActivityMessage | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const item = value as ActivityItem;
  const id = text(item.id);
  if (!id) return null;
  const message: ActivityMessage = { role: "assistant", uuid: `activity-${id}`, blocks: [], ts, timestamp };
  if (item.type === "CommandExecution") {
    const command = text(item.command);
    if (!command || (item.status !== "completed" && item.status !== "failed")) return null;
    const failed = item.status === "failed" || (typeof item.exit_code === "number" && item.exit_code !== 0);
    const output = text(item.aggregated_output) || [text(item.stdout), text(item.stderr)].filter(Boolean).join("\n");
    message.blocks = [
      { kind: "tool_use", data: {
        id, name: "command_execution", input: { command },
        tool_kind: "command", tool_summary: command, tool_detail: command,
        tool_status: failed ? "error" : "success",
      } },
      { kind: "tool_result", data: { tool_use_id: id, content: output, is_error: failed } },
    ];
    return message;
  }
  if (item.type === "SubAgentActivity") {
    const kind = text(item.kind);
    const path = text(item.agent_path);
    if (!kind || !path) return null;
    message.blocks = [{ kind: "tool_use", data: {
      id, name: "agent_activity", input: { kind, agent_path: path, agent_thread_id: text(item.agent_thread_id) },
      tool_kind: "task", tool_summary: `${kind} ${path}`, tool_detail: `${kind} ${path}`,
      tool_status: "success",
    } }];
    return message;
  }
  return null;
}
