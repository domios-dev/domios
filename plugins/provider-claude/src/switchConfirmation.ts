import type { ParsedPrompt } from "@codeterm/plugin-sdk";

export function parseSwitchConfirmationPrompt(screen: string, selection: string): ParsedPrompt | null {
  if (!selection || /\s|[\x00-\x1f]/.test(selection)) return null;
  const lines = screen.replace(/\r\n?/g, "\n").trimEnd().split("\n");
  const row = /^\s*(❯\s*)?([12])\.\s+(.+?)\s*$/;
  const second = row.exec(lines[lines.length - 1] ?? "");
  const first = row.exec(lines[lines.length - 2] ?? "");
  if (!first || !second || first[2] !== "1" || second[2] !== "2") return null;
  if (!first[1] || second[1]) return null;
  const before = lines.slice(0, -2);
  let questionIndex = -1;
  for (let index = before.length - 1; index >= 0; index--) {
    if (before[index].trim().endsWith("?")) { questionIndex = index; break; }
  }
  if (questionIndex < 0 || before.slice(questionIndex + 1).filter(line => line.trim()).length < 1) return null;
  if (before.slice(questionIndex).some(line => /[│┃┌└☐☑]|^\s*[❯>]\s*\d+\./.test(line))) return null;
  const escaped = selection.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const token = new RegExp(`(?:^|[^a-z0-9_-])${escaped}(?:$|[^a-z0-9_-])`, "i");
  if (!token.test(first[3]) || token.test(second[3])) return null;
  return { question: before[questionIndex].trim(), options: [[first[3], "1"], [second[3], "2"]] };
}
