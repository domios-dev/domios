// Keybinding matcher — language-independent via e.code for letters/digits.
//
// Format: "Mod+Shift+T", "Alt+ArrowLeft", "Ctrl+="
// "Mod" = Cmd on mac, Ctrl on win/linux (resolved at match time).
// Letters/digits use e.code (KeyT, Digit1) so layout doesn't matter.
// Special keys (Tab, ArrowLeft, =, -, etc.) use e.key.

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

interface ParsedBinding {
  mod: boolean;   // Cmd (mac) / Ctrl (win)
  ctrl: boolean;  // explicit Ctrl
  shift: boolean;
  alt: boolean;
  // code-based match for letters/digits (e.g. "KeyT", "Digit1")
  code?: string;
  // key-based match for special keys (e.g. "Tab", "=", ",")
  key?: string;
}

function parseBinding(binding: string): ParsedBinding {
  const parts = binding.split("+").map((p) => p.trim());
  const result: ParsedBinding = { mod: false, ctrl: false, shift: false, alt: false };

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
      // Key part — decide code vs key
      if (/^[a-zA-Z]$/.test(part)) {
        result.code = `Key${part.toUpperCase()}`;
      } else if (/^[0-9]$/.test(part)) {
        result.code = `Digit${part}`;
      } else {
        // Special keys: Tab, ArrowLeft, =, -, ,, etc.
        result.key = part;
      }
    }
  }

  // "Ctrl+X" → treat Ctrl as Mod (Cmd on mac, Ctrl on win)
  // Exception: "Ctrl+Tab" stays literal Ctrl (Cmd+Tab is system shortcut on mac)
  const isTabBinding = result.key?.toLowerCase() === "tab";
  if (result.ctrl && !result.mod && !isTabBinding) {
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

/** Check if a KeyboardEvent matches a binding string. */
export function matchesBinding(e: KeyboardEvent, binding: string): boolean {
  const p = getParsed(binding);

  // Check modifiers
  const modKey = IS_MAC ? e.metaKey : e.ctrlKey;
  if (p.mod !== modKey) return false;
  if (p.shift !== e.shiftKey) return false;
  if (p.alt !== e.altKey) return false;

  // On mac, if binding uses Mod (→metaKey), we don't care about ctrlKey state
  // On win, if binding uses Mod (→ctrlKey), we don't care about metaKey state
  // But explicit ctrl in binding means ctrlKey must be true even on mac
  if (p.ctrl && !e.ctrlKey) return false;

  // Check key
  if (p.code) {
    return e.code === p.code;
  }
  if (p.key) {
    return e.key === p.key || e.key.toLowerCase() === p.key.toLowerCase();
  }

  return false;
}

/** Check if we're on macOS. */
export const isMac = IS_MAC;
