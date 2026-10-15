/**
 * Self-check for `replay.mjs --health`: every provider the stub can wear must
 * resolve its default cast and its style without opening a PTY or a socket.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { DEFAULT_CASTS, castOutput } from "./replay-cast.mjs";
import { KNOWN_STUB_PROVIDERS } from "./replay-style.mjs";

function castProblem(repo, provider, castPath, readFile) {
  if (!castPath) return `no default cast for provider ${provider}`;
  const resolved = path.isAbsolute(castPath) ? castPath : path.join(repo, castPath);
  let text;
  try {
    text = readFile(resolved, "utf8");
  } catch (error) {
    return `cast for ${provider} unreadable at ${resolved}: ${error.code ?? error.message}`;
  }
  try {
    if (!castOutput(text)) return `cast for ${provider} has no output frames at ${resolved}`;
  } catch (error) {
    return `cast for ${provider} is not asciicast v2 at ${resolved}: ${error.message}`;
  }
  return null;
}

export function stubHealth({
  repo,
  casts = DEFAULT_CASTS,
  styled = KNOWN_STUB_PROVIDERS,
  readFile = readFileSync,
}) {
  const providers = [...new Set([...Object.keys(casts), ...styled])].sort();
  const problems = [];
  for (const provider of providers) {
    if (!styled.includes(provider)) problems.push(`no recorded style for provider ${provider}`);
    const problem = castProblem(repo, provider, casts[provider], readFile);
    if (problem) problems.push(problem);
  }
  return problems.length
    ? { ok: false, providers, reason: problems.join("; ") }
    : { ok: true, providers };
}
