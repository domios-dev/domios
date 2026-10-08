import { ATTACHMENT_KINDS, formatAttachmentMarker } from "@codeterm/chat-core";

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
        plain += label ?? path;
        portable += formatAttachmentMarker(path, kind, label);
        return;
      }
    }
    node.childNodes.forEach(visit);
  };
  visit(root);
  return { plain, portable };
}
