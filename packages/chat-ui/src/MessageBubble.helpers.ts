import type {
  ChatOrigin,
  NormalizedChatMessage as ChatMessage,
} from "@codeterm/chat-core";
import { shortTabRef } from "@codeterm/chat-core";
type StructuredInboundKind = NonNullable<ChatMessage["inboundKind"]>;

export const INBOUND_KIND_LABELS: Record<StructuredInboundKind, string> = {
  tab: "tab",
  report: "report",
  reminder: "reminder",
  system: "system",
  mesh: "mesh",
  telegram: "telegram",
};

export interface StructuredInbound {
  origin: ChatOrigin;
  kind: StructuredInboundKind;
}

export function structuredInboundFromMessage(message: ChatMessage): StructuredInbound | null {
  const fromLabel = message.fromLabel?.trim();
  const fromTab = shortTabRef(message.fromTab);
  if (message.inboundKind === "system") {
    return { kind: "system", origin: { kind: "system" } };
  }
  if (!fromLabel && !fromTab) return null;
  const kind = message.inboundKind ?? (message.fromMesh ? "mesh" : "tab");
  const title = kind === "mesh" && message.fromMesh
    ? `${fromLabel}@${message.fromMesh}`
    : fromLabel;
  return {
    kind,
    origin: {
      kind: "tab",
      title: title ?? "",
      tabIdShort: fromTab,
      model: message.fromModel,
      mesh: kind === "mesh" ? undefined : message.fromMesh,
      report: kind === "report",
    },
  };
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
      hasRenderableText(message.toolSummary) ||
      hasRenderableText(message.toolDetail) ||
      hasRenderableText(message.toolResult) ||
      hasRenderableValue(message.toolInput) ||
      hasRenderableValue(message.toolEdits) ||
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
