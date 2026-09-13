const MAX_TITLE_CHARS = 48;

function clip(value: string): string {
  const compact = value.replace(/\s+/g, " ").trim();
  if (compact.length <= MAX_TITLE_CHARS) return compact;
  return compact.substring(0, MAX_TITLE_CHARS - 3).trimEnd() + "...";
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  return trimmed.split(/[\\/]/).pop() || trimmed;
}

function commandHead(command: string): string {
  const tokens = command.replace(/\s+/g, " ").trim().split(" ").filter(Boolean);
  const width = tokens.length > 1 && tokens[1].charAt(0) !== "-" && tokens[1].charAt(0) !== "\"" ? 2 : 1;
  return tokens.slice(0, width).join(" ");
}

/** A short title built only from the call's canonical fields; never an inferred purpose. */
export function actionTitle(fields: Record<string, unknown>): string {
  const summary = text(fields.tool_summary);
  const detail = text(fields.tool_detail);
  if (fields.tool_kind === "command") {
    const more = /\(\+(\d+) more\)$/.exec(summary);
    return clip(commandHead(detail || summary) + (more ? ` +${more[1]}` : ""));
  }
  if (fields.tool_kind === "read" && summary) return clip(`Read ${baseName(summary)}`);
  if (fields.tool_kind === "edit") {
    const edits = Array.isArray(fields.tool_edits) ? fields.tool_edits as { path?: unknown }[] : [];
    const paths = edits.map((edit) => text(edit && edit.path)).filter((path, index, all) => path && all.indexOf(path) === index);
    if (paths.length > 0) return clip(`Patched ${baseName(paths[0])}` + (paths.length > 1 ? ` +${paths.length - 1}` : ""));
  }
  return clip(summary || "Tool activity");
}
