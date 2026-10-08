/** Scrolls the chat to a loaded message and flashes its row; false when that message is not loaded. */
export function jumpToMessage(surface: HTMLElement | null, messageId: string): boolean {
  if (!surface || !messageId) return false;
  const id = CSS.escape(messageId);
  const row = surface.querySelector<HTMLElement>(`[data-message-id="${id}"], [data-source-message-id="${id}"]`);
  if (!row) return false;
  const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
  row.scrollIntoView?.({ block: "center", behavior: reduced ? "auto" : "smooth" });
  row.removeAttribute("data-flash");
  // Reading layout restarts the flash animation when the same row is jumped to twice.
  void row.offsetWidth;
  row.setAttribute("data-flash", "");
  row.addEventListener("animationend", () => row.removeAttribute("data-flash"), { once: true });
  return true;
}
