import { attachmentTagsAsLabels } from "@codeterm/chat-core";
import { referenceMarkers } from "../../../packages/chat-ui/src/codeterm/referenceMarkers";

// Runs for every row on every streamed chunk; only the head of a long message can reach the excerpt.
const ACCESSIBLE_NAME_SCAN_CHARS = 1_000;

export function chatMessageAccessibleName(type: string, content: string, resolveTabName: (id: string) => string | undefined = () => undefined): string {
  const head = content.length > ACCESSIBLE_NAME_SCAN_CHARS ? content.slice(0, ACCESSIBLE_NAME_SCAN_CHARS) : content;
  const display = (text: string) => {
    let result = text;
    for (const marker of referenceMarkers(text).reverse()) {
      const id = marker.peer ? `mesh:${marker.peer}/${marker.id}` : marker.id;
      if (marker.kind === "tab") result = result.slice(0, marker.start) + `@${resolveTabName(id) ?? "Unknown tab"}` + result.slice(marker.end);
    }
    return attachmentTagsAsLabels(result).replace(/\s+/g, " ").trim();
  };
  let compact = display(head);
  if (compact.length <= 80 && head.length < content.length) compact = display(content);
  const excerpt = compact.length > 80 ? `${compact.slice(0, 77)}…` : compact;
  return excerpt ? `${type} message: ${excerpt}` : `${type} message`;
}
