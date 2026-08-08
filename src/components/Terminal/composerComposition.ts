export interface CompositionGate {
  active: boolean;
}

export function createCompositionGate(): CompositionGate {
  return { active: false };
}

export function beginComposition(gate: CompositionGate): void {
  gate.active = true;
}

export function endComposition(gate: CompositionGate): void {
  gate.active = false;
}

/** IME keystrokes commit the composition, never submit. WebKit delivers the
 *  committing Enter after compositionend with isComposing=false, keyCode 229. */
export function isImeKeyEvent(
  gate: CompositionGate,
  nativeIsComposing: boolean,
  keyCode: number | undefined,
): boolean {
  return gate.active || nativeIsComposing || keyCode === 229;
}

/** Style writes + forced reflow on a composing textarea disturb the IME preedit
 *  (WebKitGTK re-inserts draft prefixes); autosize waits for the commit. */
export function compositionDefersAutosize(gate: CompositionGate): boolean {
  return gate.active;
}
