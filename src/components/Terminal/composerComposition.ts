export interface CompositionGate {
  active: boolean;
  domValue: string;
}

export function createCompositionGate(): CompositionGate {
  return { active: false, domValue: "" };
}

export function beginComposition(gate: CompositionGate, domValue: string): void {
  gate.active = true;
  gate.domValue = domValue;
}

/** Intermediate IME text stays out of React state: reassigning the controlled
 *  value mid-composition makes WebKitGTK re-insert every draft prefix. */
export function deferCompositionChange(
  gate: CompositionGate,
  nativeIsComposing: boolean,
  domValue: string,
): boolean {
  if (!gate.active && !nativeIsComposing) return false;
  gate.domValue = domValue;
  return true;
}

/** IME keystrokes must commit the composition, never submit. WebKit delivers
 *  the committing Enter after compositionend with isComposing=false, keyCode 229. */
export function isImeKeyEvent(
  gate: CompositionGate,
  nativeIsComposing: boolean,
  keyCode: number | undefined,
): boolean {
  return gate.active || nativeIsComposing || keyCode === 229;
}

/** Close the gate and return the finished draft — the single buffer commit
 *  per composition. A canceled composition returns the pre-composition value. */
export function endComposition(gate: CompositionGate, domValue: string): string {
  gate.active = false;
  gate.domValue = domValue;
  return domValue;
}

/** While composing, render the DOM's own value so an unrelated re-render can
 *  never write a stale draft into the active composition. */
export function compositionRenderValue(gate: CompositionGate, committed: string): string {
  return gate.active ? gate.domValue : committed;
}
