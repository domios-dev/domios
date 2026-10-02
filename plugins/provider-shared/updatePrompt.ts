interface ParsedPrompt {
  options: [string, string][];
}

export function updatePromptSafeChoice(prompt: ParsedPrompt): string | null {
  if (prompt.options.length < 2 || !prompt.options.some(([label]) => /\bupdate\b/i.test(label))) {
    return null;
  }
  const safe = prompt.options.filter(([label]) => /\b(skip|later|postpone)\b/i.test(label));
  // Codex offers "Skip" and "Skip until next version"; the bare skip is the one-time choice.
  const bare = safe.filter(([label]) => /^\s*(skip|later|postpone)\s*$/i.test(label));
  const chosen = safe.length === 1 ? safe : bare;
  if (chosen.length !== 1) return null;
  const [label, value] = chosen[0];
  if (!value || /\bupdate\b/i.test(label)) return null;
  if (prompt.options.filter(([, candidate]) => candidate === value).length !== 1) return null;
  return value;
}
