export const CODETERM_CODEX_HOOK_MARKER_V1149 = "codeterm-codex-session-binding-v1"; // rename-bridge
export const CODETERM_CODEX_HOOK_SCRIPT_NAME_V1149 = "codeterm-session-hook.cmd"; // rename-bridge

function containsExactMarker(value: string, marker: string): boolean {
  let index = value.indexOf(marker);
  while (index !== -1) {
    const previous = value[index - 1];
    const next = value[index + marker.length];
    const isTokenChar = (character: string | undefined) => !!character && /[A-Za-z0-9_-]/.test(character);
    if (!isTokenChar(previous) && !isTokenChar(next)) return true;
    index = value.indexOf(marker, index + 1);
  }
  return false;
}

export function is1149CodexScript(contents: string): boolean {
  return containsExactMarker(contents, CODETERM_CODEX_HOOK_MARKER_V1149); // rename-bridge
}

export function commandIsOwned(command: string): boolean {
  return containsExactMarker(command, "X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER_V1149); // rename-bridge
}

export function scriptIsOwned(contents: string): boolean {
  return containsExactMarker(contents, CODETERM_CODEX_HOOK_MARKER_V1149); // rename-bridge
}
