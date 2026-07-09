import type {
  ChatOrigin,
  NormalizedChatMessage as ChatMessage,
} from "@codeterm/chat-core";
import { isShellToolName } from "./MarkdownBody";

export type CuratedToolKind =
  | "exec"
  | "read_file"
  | "write_file"
  | "codeterm"
  | "mem_search"
  | "spawn_agent";

type StructuredInboundKind = NonNullable<ChatMessage["inboundKind"]>;

export const INBOUND_KIND_LABELS: Record<StructuredInboundKind, string> = {
  pane: "pane",
  report: "report",
  reminder: "reminder",
  mesh: "mesh",
  telegram: "telegram",
};

export interface StructuredInbound {
  origin: ChatOrigin;
  kind: StructuredInboundKind;
}

export function structuredInboundFromMessage(message: ChatMessage): StructuredInbound | null {
  const fromLabel = message.fromLabel?.trim();
  if (!fromLabel) return null;
  const kind = message.inboundKind ?? (message.fromMesh ? "mesh" : "pane");
  const title = kind === "mesh" && message.fromMesh
    ? `${fromLabel}@${message.fromMesh}`
    : fromLabel;
  return {
    kind,
    origin: {
      kind: "pane",
      title,
      paneIdShort: message.fromPane ?? "",
      model: message.fromModel,
      mesh: kind === "mesh" ? undefined : message.fromMesh,
      report: kind === "report",
    },
  };
}

export function parsedToolInput(message: ChatMessage): Record<string, unknown> {
  if (message.toolInput) return message.toolInput;
  if (!message.toolArgs) return {};
  try {
    const parsed = JSON.parse(message.toolArgs);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : {};
  } catch {
    return {};
  }
}

export function textArg(input: Record<string, unknown>, keys: string[]): string | null {
  for (const key of keys) {
    const value = input[key];
    if (typeof value === "string" && value.trim()) return value;
    if (Array.isArray(value) && value.length > 0) return value.map(String).join(" ");
  }
  return null;
}

export function contentSize(content: string): string {
  const bytes = new TextEncoder().encode(content).length;
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

export function previewLine(text: string, max = 90): string {
  const line = text.replace(/\r/g, "").split("\n").find((part) => part.trim())?.trim() ?? "";
  return line.length > max ? `${line.slice(0, max)}...` : line;
}

function curatedToolKind(name: string | undefined): CuratedToolKind | null {
  const normalized = (name ?? "").trim().toLowerCase();
  switch (normalized) {
    case "exec":
    case "read_file":
    case "write_file":
    case "codeterm":
    case "mem_search":
    case "spawn_agent":
      return normalized;
    default:
      return null;
  }
}

export function resolveCuratedToolKind(name: string | undefined): CuratedToolKind | null {
  const direct = curatedToolKind(name);
  if (direct) return direct;
  const normalized = (name ?? "").trim().toLowerCase();
  if (isShellToolName(name)) return "exec";
  if (normalized === "read") return "read_file";
  if (normalized === "write") return "write_file";
  return null;
}

export function toolDisplayLabel(toolName: string): string {
  if (isShellToolName(toolName) || toolName === "shell_command" || toolName === "exec_command") {
    return "shell";
  }
  return toolName;
}

function hasRenderableValue(value: unknown): boolean {
  if (value == null) return false;
  if (typeof value === "string") return value.trim().length > 0;
  if (Array.isArray(value)) return value.length > 0;
  if (typeof value === "object") return Object.keys(value).length > 0;
  return true;
}

function hasRenderableText(value: string | undefined): boolean {
  return typeof value === "string" && value.trim().length > 0;
}

export function isRenderableMessage(message: ChatMessage): boolean {
  if (message.type === "tool_call") {
    return (
      hasRenderableText(message.content) ||
      hasRenderableText(message.toolName) ||
      hasRenderableText(message.toolArgs) ||
      hasRenderableText(message.toolResult) ||
      hasRenderableValue(message.toolInput) ||
      Boolean(message.interactive?.length)
    );
  }

  if (message.type === "tool_result") {
    return hasRenderableText(message.content) || hasRenderableText(message.toolResult);
  }

  return true;
}

export function hasMeaningfulToolResult(content: string): boolean {
  return content
    .split("\n")
    .map((line) => line.trim())
    .some((line) => line.length > 0 && !/^(?:->|→|⎿)+$/.test(line));
}
