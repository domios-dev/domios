import type { ParsedSessionRecord } from "@codeterm/plugin-sdk";

export type SessionRecordInput = string | readonly ParsedSessionRecord[];

export function* sessionRecords<T>(input: SessionRecordInput, accepts?: (line: string) => boolean): Generator<{ row: number; value: T }> {
  if (typeof input !== "string") {
    for (const record of input) yield { row: record.row, value: record.value as T };
    return;
  }
  const lines = String(input || "").split("\n");
  for (let row = 0; row < lines.length; row++) {
    const line = lines[row];
    if (!line || (accepts && !accepts(line))) continue;
    let value: T;
    try { value = JSON.parse(line) as T; } catch { continue; }
    yield { row, value };
  }
}
