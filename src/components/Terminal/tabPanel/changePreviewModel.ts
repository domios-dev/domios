import type { LocalArtifactKind } from "@codeterm/chat-ui";

export function changePreviewKind(path: string): LocalArtifactKind {
  if (/\.(html|htm)$/i.test(path)) return "page";
  if (/\.(png|jpe?g|gif|webp|bmp|avif|svg)$/i.test(path)) return "image";
  return "document";
}

export function changePreviewFailure(error: string): string {
  if (/binary|UTF-8/i.test(error)) return "Binary or non-UTF-8 file: text preview is unavailable.";
  if (/exceeds|too large/i.test(error)) return "This file is too large to preview.";
  if (/resolve preview file|not found|os error 2|os error 3|404/i.test(error)) return "This file is unavailable. It may have been deleted or moved since the edit.";
  return "This file could not be read. Read its recorded diff or open it in an editor.";
}
