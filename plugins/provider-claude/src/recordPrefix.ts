export function storedRecordType(line: string): string | undefined {
  const tokens = /"type"\s*:\s*"([^"\\]*)"|"(?:\\.|[^"\\])*"|[\{\[]/g;
  const opening = tokens.exec(line);
  if (opening?.[0] !== "{") return undefined;
  for (let token = tokens.exec(line); token; token = tokens.exec(line)) {
    if (token[1] !== undefined) return token[1];
    if (token[0] === "{" || token[0] === "[") return undefined;
  }
  return undefined;
}

export function shouldParseClaudeRecord(line: string, delta: boolean): boolean {
  const type = storedRecordType(line);
  if (type === undefined || type === "user" || type === "assistant") return true;
  if (!delta) return false;
  if (["system", "ai-title", "edited_text_file", "attachment"].includes(type)) return true;
  // Observation and background records are accepted independently of their top-level type.
  return /"(?:permissionMode|isApiErrorMessage|toolUseResult|message|attachment)"\s*:|\\u/.test(line);
}
