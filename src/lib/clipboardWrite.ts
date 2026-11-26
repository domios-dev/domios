/**
 * Write plain text to the viewing machine's clipboard.
 * Tries the async Clipboard API first and falls back to a capture-phase copy
 * event for hosts without `navigator.clipboard` (for example insecure HTTP
 * mirrors) or where async writes are rejected.
 */
export async function writeViewerClipboardText(
  text: string,
  doc: Document = document,
): Promise<boolean> {
  const clipboard = globalThis.navigator?.clipboard ?? doc.defaultView?.navigator?.clipboard;
  if (clipboard?.writeText) {
    try {
      await clipboard.writeText(text);
      return true;
    } catch {
      // Fall through to execCommand("copy") when the async API rejects.
    }
  }

  let handled = false;
  const onCopy = (event: ClipboardEvent) => {
    if (!event.clipboardData) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    event.clipboardData.setData("text/plain", text);
    handled = true;
  };
  doc.addEventListener("copy", onCopy, true);
  try {
    return Boolean(doc.execCommand?.("copy") && handled);
  } catch {
    return false;
  } finally {
    doc.removeEventListener("copy", onCopy, true);
  }
}
