/**
 * Recorded real-session boot screens for the stub replayer. Detection
 * fingerprints come from these bytes, so a provider the stub can wear without
 * a cast here boots as a blank terminal that no detector recognises.
 *
 * Shared with the parity gate so the map and the assertion cannot drift.
 */

export const DEFAULT_CASTS = {
  claude:
    "testbench/replays/claude/geometry_smoke_v1-20260621-geometry-smoke-v2-claude-geometry_smoke_v1-100x30/raw.cast",
  codex:
    "testbench/replays/codex/codex_prompt_idle_regression_v1-20260621-codex-prompt-idle-v1/raw.cast",
  opencode:
    "testbench/replays/opencode/geometry_smoke_v1-20260621-geometry-smoke-v2-opencode-geometry_smoke_v1-100x30/raw.cast",
  cursor:
    "testbench/replays/cursor/geometry_smoke_v1-20260621-geometry-smoke-v2-cursor-geometry_smoke_v1-100x30/raw.cast",
};

export const DEFAULT_REPLAY_BYTE_CAP = 12_000;

export function defaultCastFor(provider) {
  return Object.hasOwn(DEFAULT_CASTS, String(provider)) ? DEFAULT_CASTS[provider] : null;
}

export function stripTerminalQueries(data) {
  return data
    .replaceAll("[c", "")
    .replaceAll("[6n", "")
    .replaceAll("]10;?\\", "")
    .replaceAll("]11;?\\", "");
}

/** Concatenated `o` frames of an asciicast v2 file, queries stripped, capped. */
export function castOutput(castText, byteCap = DEFAULT_REPLAY_BYTE_CAP) {
  const lines = castText.split("\n").slice(1);
  let out = "";
  let bytesWritten = 0;
  for (const line of lines) {
    if (!line.trim()) continue;
    const [, kind, data] = JSON.parse(line);
    if (kind !== "o") continue;
    const chunk = stripTerminalQueries(data);
    const remaining = byteCap - bytesWritten;
    if (remaining <= 0) break;
    const visible = chunk.slice(0, remaining);
    out += visible;
    bytesWritten += Buffer.byteLength(visible);
    if (bytesWritten >= byteCap) break;
  }
  return out;
}
