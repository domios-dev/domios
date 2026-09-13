import { actionTitle, baseName } from "./actionTitle";
import { changedPaths, codexFileDiffs, diffEdits } from "./fileDiffs";
import { extensionResult, imageViewResult } from "./richResults";

interface ActivityItem {
  type?: unknown;
  id?: unknown;
  changes?: unknown;
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
  if (item.type === "FileChange") {
    const paths = changedPaths(item.changes);
    if (paths.length === 0 || (item.status !== "completed" && item.status !== "failed")) return null;
    const failed = item.status === "failed";
    const diffs = codexFileDiffs(item.changes);
    const fields = {
      tool_kind: "edit",
      tool_summary: paths[0] + (paths.length > 1 ? ` +${paths.length - 1}` : ""),
      tool_detail: paths.join("\n"),
      tool_edits: diffEdits(diffs),
    };
    message.blocks = [
      { kind: "tool_use", data: {
        id, name: "file_change", input: { paths },
        ...fields, tool_title: actionTitle({ ...fields, tool_edits: paths.map((path) => ({ path })) }), ...flow,
        ...(diffs.length > 0 ? { file_diffs: diffs } : {}),
        tool_status: failed ? "error" : "success",
      } },
      { kind: "tool_result", data: { tool_use_id: id, content: [text(item.stdout), text(item.stderr)].filter(Boolean).join("\n"), is_error: failed } },
    ];
    return message;
  }
  if (item.type === "ImageView") return imageViewResult(value as Record<string, unknown>, ts, timestamp, turnId);
  if (item.type === "Extension") return extensionResult(value as Record<string, unknown>, ts, timestamp, turnId);
  if (item.type === "SubAgentActivity") {
    const kind = text(item.kind);
    const path = text(item.agent_path);
    if (!kind || !path) return null;
    message.blocks = [{ kind: "tool_use", data: {
      id, name: "agent_activity", input: { kind, agent_path: path, agent_thread_id: text(item.agent_thread_id) },
      tool_kind: "task", tool_summary: `${kind} ${path}`, tool_detail: `${kind} ${path}`,
      tool_title: actionTitle({ tool_summary: `${kind} ${baseName(path)}` }), ...flow,
      tool_status: "success",
    } }];
    return message;
  }
  return null;
}
