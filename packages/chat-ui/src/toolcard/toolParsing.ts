import type { EditPair, NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import { parseCodexPatch, patchTextFromToolInput } from "@codeterm/chat-core";

export function extractFilePath(message: ChatMessage): string | null {
  const input = message.toolInput ?? parseArgs(message.toolArgs);
  const patch = patchTextFromToolInput(input, message.toolArgs);
  const patchPath = patch ? parseCodexPatch(patch)[0]?.path : undefined;
  if (patchPath) return patchPath;
  for (const key of ["file_path", "filePath", "path"]) {
    const value = input?.[key];
    if (typeof value === "string" && value.trim()) return value;
  }
  return null;
}

export function extractEdits(message: ChatMessage): EditPair[] {
  const input = message.toolInput ?? parseArgs(message.toolArgs);
  const patch = patchTextFromToolInput(input, message.toolArgs);
  if (patch) return parseCodexPatch(patch).flatMap((file) => file.edits);
  if (!input) return [];
  const pair = (value: Record<string, unknown>): EditPair | null => {
    const old = value.old_string ?? value.oldString;
    const next = value.new_string ?? value.newString;
    if (typeof old !== "string" && typeof next !== "string") return null;
    return { old: typeof old === "string" ? old : "", new: typeof next === "string" ? next : "" };
  };
  if (Array.isArray(input.edits)) return input.edits.map((edit) => pair(edit as Record<string, unknown>)).filter((edit): edit is EditPair => !!edit);
  const single = pair(input);
  return single ? [single] : [];
}

export function extractCommand(message: ChatMessage): string | null {
  const input = message.toolInput ?? parseArgs(message.toolArgs);
  for (const key of ["command", "cmd"]) {
    const value = input?.[key];
    if (typeof value === "string" && value.trim()) return value;
  }
  return message.toolArgs?.trim() || null;
}

function parseArgs(args?: string): Record<string, unknown> | null {
  if (!args) return null;
  try {
    const parsed: unknown = JSON.parse(args);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed as Record<string, unknown> : null;
  } catch {
    return null;
  }
}
