export type FallbackKeyEvent = Pick<KeyboardEvent, "altKey" | "ctrlKey" | "isComposing" | "key" | "metaKey" | "shiftKey"> & {
  keyCode?: number;
};

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

  if (e.key.length === 1 && e.key.charCodeAt(0) >= 0x20 && e.key.charCodeAt(0) <= 0x7e) return e.key;

  return "";
}
