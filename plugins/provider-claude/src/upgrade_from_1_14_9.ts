export const CODETERM_CLAUDE_HOOK_MARKER_V1149 = "codeterm-hook-v3"; // rename-bridge

export function commandIsOwned(command: string): boolean {
  const marker = "X-CodeTerm-Hook: " + CODETERM_CLAUDE_HOOK_MARKER_V1149; // rename-bridge
  const index = command.indexOf(marker);
  if (index === -1) return false;
  const isTokenChar = (value: string | undefined) => !!value && /[A-Za-z0-9_-]/.test(value);
  return !isTokenChar(command[index - 1]) && !isTokenChar(command[index + marker.length]);
}
