export type FallbackKeyEvent = Pick<KeyboardEvent, "altKey" | "ctrlKey" | "isComposing" | "key" | "metaKey" | "shiftKey"> & {
  keyCode?: number;
};

type FallbackDispatchEvent = FallbackKeyEvent & Pick<KeyboardEvent, "preventDefault">;

const interactiveRoles = new Set([
  "button", "checkbox", "combobox", "link", "listbox", "menuitem", "menuitemcheckbox", "menuitemradio",
  "option", "radio", "searchbox", "slider", "spinbutton", "switch", "tab", "textbox", "treeitem",
]);

const interactiveSelector = "a[href], button, input, option, select, summary, textarea";

export function blocksTerminalFallback(active: Element | null): boolean {
  if (!active || active.classList.contains("xterm-helper-textarea")) return true;
  if (active.matches(interactiveSelector) || active.closest(interactiveSelector)) return true;
  if (active instanceof HTMLElement && active.isContentEditable) return true;
  const role = active.closest("[role]")?.getAttribute("role");
  return role !== null && role !== undefined && interactiveRoles.has(role);
}

export function forwardTerminalFallback(event: FallbackDispatchEvent, active: Element | null, write: (data: string) => void): boolean {
  if (blocksTerminalFallback(active)) return false;
  const data = terminalFallbackData(event);
  if (!data) return false;
  event.preventDefault();
  write(data);
  return true;
}

export function terminalFallbackData(e: FallbackKeyEvent): string {
  if (e.isComposing || e.keyCode === 229 || e.key === "Process" || e.key === "Dead") return "";

  if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.key.length === 1) {
    const code = e.key.toUpperCase().charCodeAt(0) - 64;
    return code > 0 && code < 32 ? String.fromCharCode(code) : "";
  }

  if (e.ctrlKey || e.altKey || e.metaKey) return "";

  if (e.key === "Enter") return "\r";
  if (e.key === "Backspace") return "\x7f";
  if (e.key === "Tab") return "\t";
  if (e.key === "Escape") return "\x1b";
  if (e.key === "ArrowUp") return "\x1b[A";
  if (e.key === "ArrowDown") return "\x1b[B";
  if (e.key === "ArrowRight") return "\x1b[C";
  if (e.key === "ArrowLeft") return "\x1b[D";
  if (e.key === "Home") return "\x1b[H";
  if (e.key === "End") return "\x1b[F";
  if (e.key === "Delete") return "\x1b[3~";

  // Keep composition text on xterm's helper textarea because forwarding intermediate non-ASCII keydowns duplicates input in the PTY.
  if (e.key.length === 1 && e.key.charCodeAt(0) >= 0x20 && e.key.charCodeAt(0) <= 0x7e) return e.key;

  return "";
}
