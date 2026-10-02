import type { ParsedPrompt } from "@codeterm/plugin-sdk";

export function updatePromptSafeChoice(prompt: ParsedPrompt): string | null {
  if (prompt.options.length < 2 || !prompt.options.some(([label]) => /\bupdate\b/i.test(label))) {
    return null;
  }
  const safe = prompt.options.filter(([label]) => /\b(skip|later|postpone)\b/i.test(label));
  if (safe.length !== 1) return null;
  const [label, value] = safe[0];
  if (!value || /\bupdate\b/i.test(label)) return null;
  if (prompt.options.filter(([, candidate]) => candidate === value).length !== 1) return null;
  return value;
}
