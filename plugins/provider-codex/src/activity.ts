import { actionTitle, baseName } from "./actionTitle";
import { collabCallRow, reasoningItemRow, subAgentRow } from "./agentActivity";

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
  parsed_cmd?: unknown;
}

function parsedReadTitle(value: unknown): string {
  if (!Array.isArray(value) || value.length === 0) return "";
  const names: string[] = [];
  for (const entry of value) {
    if (!entry || typeof entry !== "object") return "";
    const parsed = entry as { type?: unknown; name?: unknown; path?: unknown };
    const name = text(parsed.name) || baseName(text(parsed.path));
    if (parsed.type !== "read" || !name) return "";
    if (names.indexOf(name) === -1) names.push(name);
  }
  return actionTitle({ tool_summary: `Read ${names[0]}` + (names.length > 1 ? ` +${names.length - 1}` : "") });
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

function commandText(value: unknown): string {
  if (typeof value === "string") return value;
  if (!Array.isArray(value) || value.length === 0 || !value.every((argument) => typeof argument === "string")) return "";
  const argv = value as string[];
  if (!argv[0].trim()) return "";
  return argv.map((argument) => argument === "" || /[\s"]/.test(argument) ? JSON.stringify(argument) : argument).join(" ");
}

export function completedActivity(value: unknown, ts: number | null, timestamp: string | null, turnId?: unknown): ActivityMessage | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const item = value as ActivityItem;
  const id = text(item.id);
  if (!id) return null;
  const flow = text(turnId) ? { tool_flow_id: text(turnId) } : {};
  const message: ActivityMessage = { role: "assistant", uuid: `activity-${id}`, blocks: [], ts, timestamp };
  if (item.type === "CommandExecution") {
    const command = commandText(item.command);
    if (!command || (item.status !== "completed" && item.status !== "failed")) return null;
    const failed = item.status === "failed" || (typeof item.exit_code === "number" && item.exit_code !== 0);
    const output = text(item.aggregated_output) || [text(item.stdout), text(item.stderr)].filter(Boolean).join("\n");
    const fields = { tool_kind: "command", tool_summary: command, tool_detail: command };
    message.blocks = [
      { kind: "tool_use", data: {
        id, name: "command_execution", input: { command, ...(Array.isArray(item.command) ? { argv: item.command } : {}) },
        ...fields, tool_title: parsedReadTitle(item.parsed_cmd) || actionTitle(fields), ...flow,
        // The exec call that ran this command is already in the transcript; no source id links the two.
        tool_echo: true,
        tool_status: failed ? "error" : "success",
      } },
      { kind: "tool_result", data: { tool_use_id: id, content: output, is_error: failed } },
    ];
    return message;
  }
  const fields = value as Record<string, unknown>;
  if (item.type === "SubAgentActivity") return subAgentRow(fields, id, turnId, ts, timestamp) as ActivityMessage | null;
  if (item.type === "CollabAgentToolCall") return collabCallRow(fields, turnId, ts, timestamp) as ActivityMessage | null;
  if (item.type === "Reasoning") return reasoningItemRow(fields, ts, timestamp) as ActivityMessage | null;
  return null;
}
