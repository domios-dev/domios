import type { ParsedPrompt } from "@codeterm/plugin-sdk";

export interface ExternalImportsPrompt extends ParsedPrompt {
  focusedIndex: number;
}

export function claudeExternalImportsPrompt(screen: string): ExternalImportsPrompt | null {
  const rows = String(screen || "").replace(/\r\n?/g, "\n").split("\n")
    .map((row) => row.replace(/^[\s│┃║─━═]+|[\s│┃║─━═]+$/g, ""));
  while (rows.length && !rows[rows.length - 1]) rows.pop();
  const footer = rows.length - 1;
  if (!/^Enter\s+to\s+confirm\s*[·•]\s*Esc\s+to\s+cancel$/i.test(rows[footer] || "")) return null;
  const options: Array<{ label: string; focused: boolean; line: number }> = [];
  for (let line = footer - 1; line >= 0; line--) {
    const row = rows[line];
    if (!row) continue;
    const match = /^([❯>])?\s*((?:No|Yes),\s*(?:disable|allow)\s+external\s+imports)$/i.exec(row);
    if (!match) break;
    options.unshift({ label: match[2], focused: Boolean(match[1]), line });
  }
  if (options.length !== 2 || options.filter((option) => option.focused).length !== 1) return null;
  if (!/^No,\s*disable/i.test(options[0].label) || !/^Yes,\s*allow/i.test(options[1].label)) return null;
  const before = rows.slice(0, options[0].line);
  let title = -1;
  for (let index = 0; index < before.length; index++) {
    if (/^Allow\s+external\s+CLAUDE\.md\s+file\s+imports\?$/i.test(before[index])) title = index;
  }
  if (title < 0) return null;
  const context = before.slice(title + 1);
  const imports = context.findIndex((row) => /^External\s+imports:$/i.test(row));
  if (imports < 0 || !/^(?:[A-Za-z]:[\\/]|\/|~[\\/]|\\\\)/.test(context[imports + 1] || "")) return null;
  if (context.some((row) => /^[❯>]/.test(row))) return null;
  return {
    question: "Claude is asking whether to allow external CLAUDE.md imports",
    options: options.map((option, index) => [option.label, String(index + 1)]),
    focusedIndex: options.findIndex((option) => option.focused),
  };
}
