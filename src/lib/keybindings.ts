// Keybinding matcher keys every chord off `e.code` (layout-safe).
// `Mod` resolves to Cmd on mac, Ctrl on win/linux.

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

interface ParsedBinding {
  mod: boolean;   // Cmd (mac) / Ctrl (win)
  ctrl: boolean;  // explicit Ctrl
  shift: boolean;
  alt: boolean;
  code: string | null;
}

const SYMBOL_CODES: Record<string, string> = {
  "=": "Equal",
  "+": "Equal",
  "-": "Minus",
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
  ";": "Semicolon",
  "'": "Quote",
  "`": "Backquote",
  "[": "BracketLeft",
  "]": "BracketRight",
  "\\": "Backslash",
};

function keyCode(part: string): string {
  if (/^[a-zA-Z]$/.test(part)) return `Key${part.toUpperCase()}`;
  if (/^[0-9]$/.test(part)) return `Digit${part}`;
  if (SYMBOL_CODES[part]) return SYMBOL_CODES[part];
  const lower = part.toLowerCase();
  if (lower === "esc") return "Escape";
  if (lower === "space") return "Space";
  return part.charAt(0).toUpperCase() + part.slice(1);
}

function parseBinding(binding: string): ParsedBinding {
  // "Ctrl++" names the plus key; a plain split would drop it.
  const parts = binding.replace(/\+\+$/, "+=").split("+").map((p) => p.trim()).filter(Boolean);
  const result: ParsedBinding = { mod: false, ctrl: false, shift: false, alt: false, code: null };

  for (const part of parts) {
    const lower = part.toLowerCase();
    if (lower === "mod" || lower === "cmd" || lower === "super") {
      result.mod = true;
    } else if (lower === "ctrl" || lower === "control") {
      result.ctrl = true;
    } else if (lower === "shift") {
      result.shift = true;
    } else if (lower === "alt" || lower === "opt" || lower === "option") {
      result.alt = true;
    } else {
      result.code = keyCode(part);
    }
  }

  // "Ctrl+X" means Mod (Cmd on mac); "Ctrl+Tab" stays literal Ctrl because Cmd+Tab belongs to macOS.
  if (result.ctrl && !result.mod && result.code !== "Tab") {
    result.mod = true;
    result.ctrl = false;
  }

  return result;
}

const cache = new Map<string, ParsedBinding>();

function getParsed(binding: string): ParsedBinding {
  let p = cache.get(binding);
  if (!p) {
    p = parseBinding(binding);
    cache.set(binding, p);
  }
  return p;
}

type ChordEvent = Pick<KeyboardEvent, "code" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">;

/** Platform-explicit matcher; `matchesBinding` binds it to the running platform. */
export function matchesBindingOn(e: ChordEvent, binding: string, mac: boolean): boolean {
  const p = getParsed(binding);
  if (!p.code || e.code !== p.code) return false;
  if (p.shift !== e.shiftKey || p.alt !== e.altKey) return false;
  if (mac) {
    // A Mod binding ignores Ctrl on mac; an explicit Ctrl still requires it.
    return p.mod === e.metaKey && (!p.ctrl || e.ctrlKey);
  }
  // Ctrl is the only Mod off mac, so Mod and explicit Ctrl name the same key.
  return (p.mod || p.ctrl) === e.ctrlKey;
}

/** Check if a KeyboardEvent matches a binding string. */
export function matchesBinding(e: KeyboardEvent, binding: string): boolean {
  return matchesBindingOn(e, binding, IS_MAC);
}

/** A bare Ctrl+letter chord, which a terminal reads as a control character off mac. */
export function isControlCharacterChord(binding: string): boolean {
  const p = getParsed(binding);
  return p.mod && !p.shift && !p.alt && /^Key[A-Z]$/.test(p.code ?? "");
}

/** Check if we're on macOS. */
export const isMac = IS_MAC;
