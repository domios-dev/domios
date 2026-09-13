export interface CanonicalToolFields {
  kind: string;
  summary: string;
  detail: string;
  edits: Array<{ path: string; old: string; new: string }>;
}

export function canonicalToolFields(name: string, input: unknown): CanonicalToolFields {
  const tool = name.trim();
  const value = input && typeof input === "object" && !Array.isArray(input)
    ? input as Record<string, unknown>
    : {};
  const text = (key: string): string => typeof value[key] === "string" ? value[key] as string : "";
  const command = text("command");
  const path = text("file_path") || text("filePath") || text("path");
  const pattern = text("pattern");
  const edits = extractCanonicalEdits(value);
  const lower = tool.toLowerCase();
  if (lower === "bash" || lower === "shell" || lower === "exec") {
    return { kind: "command", summary: cleanToolText(command, "Command"), detail: command, edits: [] };
  }
  if (lower === "edit" || lower === "multiedit" || lower === "write") {
    return { kind: "edit", summary: path || "File edit", detail: path, edits: edits };
  }
  if (lower === "read") {
    return { kind: "read", summary: path || "Read file", detail: path, edits: [] };
  }
  if (lower === "glob" || lower === "grep" || lower === "search") {
    return { kind: "search", summary: cleanToolText(pattern || path, "Search"), detail: pattern || path, edits: [] };
  }
  return {
    kind: "generic",
    summary: cleanToolText(tool, "Tool"),
    detail: tool,
    edits: edits,
  };
}

export function toolDescription(input: unknown): string {
  if (!input || typeof input !== "object" || Array.isArray(input)) return "";
  const value = (input as Record<string, unknown>).description;
  return typeof value === "string" ? value.replace(/[\r\n\t]+/g, " ").trim() : "";
}

function extractCanonicalEdits(input: Record<string, unknown>): Array<{ path: string; old: string; new: string }> {
  const parentPath = typeof input.file_path === "string" ? input.file_path : typeof input.filePath === "string" ? input.filePath : typeof input.path === "string" ? input.path : "";
  const pair = (value: unknown, fallbackPath: string): { path: string; old: string; new: string } | null => {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const item = value as Record<string, unknown>;
    const oldValue = typeof item.old_string === "string" ? item.old_string : item.oldString;
    const newValue = typeof item.new_string === "string" ? item.new_string : item.newString;
    if (typeof oldValue !== "string" && typeof newValue !== "string") return null;
    const path = typeof item.file_path === "string" ? item.file_path : typeof item.filePath === "string" ? item.filePath : typeof item.path === "string" ? item.path : fallbackPath;
    return { path: path, old: typeof oldValue === "string" ? oldValue : "", new: typeof newValue === "string" ? newValue : "" };
  };
  if (Array.isArray(input.edits)) return input.edits.map((edit) => pair(edit, parentPath)).filter((e): e is { path: string; old: string; new: string } => e !== null);
  const single = pair(input, parentPath);
  return single ? [single] : [];
}

function cleanToolText(value: string, fallback: string): string {
  const text = String(value || "").replace(/[\r\n\t]+/g, " ").trim();
  if (!text) return fallback;
  return text.length > 120 ? text.slice(0, 117) + "..." : text;
}
