import type { ClaudeEntry } from "./session";

const IMAGE_GEOMETRY = /^\[Image: original \d+x\d+, displayed at \d+x\d+\. Multiply coordinates by \d+(?:\.\d+)? to map to original image\.\]$/;

export function isImageDisplayMetadata(entry: ClaudeEntry): boolean {
  return entry.type === "user" && entry.isMeta === true
    && typeof entry.message?.content === "string"
    && IMAGE_GEOMETRY.test(entry.message.content.trim());
}
