export function toolDurationLabel(durationMs: number): string {
  const seconds = Math.floor(Math.max(0, durationMs) / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
}
