import { baseName } from "./actionTitle";
import type { ActivityMessage } from "./activity";

type Source = Record<string, unknown>;

interface RichResult {
  kind: "web" | "image";
  origin?: "generated" | "viewed";
  query?: string;
  resultCount?: number;
  action?: string;
  url?: string;
  pattern?: string;
  path?: string;
  status?: string;
  prompt?: string;
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function record(value: unknown): Source | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Source : null;
}

function webFields(source: Source): RichResult {
  const action = record(source.action);
  const rich: RichResult = { kind: "web" };
  const query = text(source.query) || text(action?.query);
  if (query) rich.query = query;
  const actionName = text(source.action) || text(action?.type);
  if (actionName) rich.action = actionName;
  if (text(action?.url)) rich.url = text(action?.url);
  if (text(action?.pattern)) rich.pattern = text(action?.pattern);
  if (Array.isArray(source.results)) rich.resultCount = source.results.length;
  return rich;
}

function resultMessage(uuid: string, id: string, name: string, rich: RichResult, summary: string, detail: string, failed: boolean, turnId: unknown, ts: number | null, timestamp: string | null): ActivityMessage {
  return {
    role: "assistant", uuid, ts, timestamp,
    blocks: [{ kind: "tool_use", data: {
      id, name, tool_kind: rich.kind, tool_summary: summary, tool_title: summary,
      ...(detail ? { tool_detail: detail } : {}),
      tool_rich_result: rich,
      ...(text(turnId) ? { tool_flow_id: text(turnId) } : {}),
      tool_status: failed ? "error" : "success",
    } }],
  };
}

export function webSearchEndResult(payload: unknown, ts: number | null, timestamp: string | null): ActivityMessage | null {
  const source = record(payload);
  const id = text(source?.call_id);
  if (!source || !id) return null;
  const rich = webFields(source);
  const summary = rich.query || rich.url || "";
  if (!summary) return null;
  return resultMessage(`result-${id}`, id, "web_search", rich, summary, summary, false, source.turn_id, ts, timestamp);
}

export function imageGenerationEndResult(payload: unknown, ts: number | null, timestamp: string | null): ActivityMessage | null {
  const source = record(payload);
  const id = text(source?.call_id);
  if (!source || !id) return null;
  const rich: RichResult = { kind: "image", origin: "generated" };
  const status = text(source.status);
  const path = text(source.saved_path);
  const prompt = text(source.revised_prompt);
  if (status) rich.status = status;
  if (path) rich.path = path;
  if (prompt) rich.prompt = prompt;
  const summary = (path && baseName(path)) || prompt || status || "image generation";
  return resultMessage(`result-${id}`, id, "image_generation", rich, summary, path || prompt, status === "failed" || status === "error", source.turn_id, ts, timestamp);
}

export function imageViewResult(item: Source, ts: number | null, timestamp: string | null, turnId?: unknown): ActivityMessage | null {
  const id = text(item.id);
  const path = text(item.path);
  if (!id || !path) return null;
  const rich: RichResult = { kind: "image", origin: "viewed", path };
  return resultMessage(`activity-${id}`, id, "image_view", rich, baseName(path) || path, path, false, turnId, ts, timestamp);
}

export function extensionResult(item: Source, ts: number | null, timestamp: string | null, turnId?: unknown): ActivityMessage | null {
  const id = text(item.id);
  if (!id || !text(item.kind).startsWith("web.")) return null;
  const rich = webFields(item);
  const summary = rich.query || rich.url || rich.action || "";
  if (!summary) return null;
  return resultMessage(`activity-${id}`, id, "extension", rich, summary, summary, false, turnId, ts, timestamp);
}
