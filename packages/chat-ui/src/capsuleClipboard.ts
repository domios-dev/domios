import { ATTACHMENT_KINDS, DOMIOS_TEXT_MIME, formatAttachmentMarker, visibleAttachmentLabel } from "@codeterm/chat-core";

/** Copied user text in two flavours: `plain` names each capsule by its label, `portable` keeps its attachment tag. */
export function capsuleSelectionText(root: Node): { plain: string; portable: string } {
  let plain = "";
  let portable = "";
  const visit = (node: Node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      plain += node.nodeValue ?? "";
      portable += node.nodeValue ?? "";
      return;
    }
    if (node instanceof HTMLElement) {
      if (node.hasAttribute("data-copy-skip")) return;
      const { attachmentPath: path, attachmentKind, attachmentLabel: label } = node.dataset;
      const kind = ATTACHMENT_KINDS.find((candidate) => candidate === attachmentKind);
      if (path && kind) {
        plain += label ? visibleAttachmentLabel(label) : path;
        portable += formatAttachmentMarker(path, kind, label);
        return;
      }
    }
    node.childNodes.forEach(visit);
  };
  visit(root);
  return { plain, portable };
}

/**
 * Writes both flavours from a button: a synthetic copy event carries the custom type, which the async
 * Clipboard API cannot; false when the host refused, after a plain-text fallback.
 */
export async function writeClipboardFlavours({ plain, portable }: { plain: string; portable: string }, doc: Document = document): Promise<boolean> {
  let handled = false;
  const onCopy = (event: ClipboardEvent) => {
    if (!event.clipboardData) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    event.clipboardData.setData("text/plain", plain);
    event.clipboardData.setData(DOMIOS_TEXT_MIME, portable);
    handled = true;
  };
  doc.addEventListener("copy", onCopy, true);
  try {
    if (doc.execCommand?.("copy") && handled) return true;
  } catch {
    // The fallback below covers hosts without execCommand.
  } finally {
    doc.removeEventListener("copy", onCopy, true);
  }
  const clipboard = doc.defaultView?.navigator.clipboard;
  if (!clipboard) return false;
  try {
    await clipboard.writeText(plain);
    return true;
  } catch {
    return false;
  }
}
