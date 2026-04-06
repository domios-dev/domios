import { invoke } from "@tauri-apps/api/core";

export interface PasteOptions {
  /** Skip text items — used on macOS where xterm handles text natively via Cmd+V. */
  skipText?: boolean;
}

/**
 * Paste clipboard contents into a PTY.
 *
 * Behavior:
 * - Text: written directly to PTY as-is (unless skipText is true)
 * - Image: saved to temp via save_clipboard_image, path written to PTY
 * - File (non-image): saved to temp via save_clipboard_file, path written to PTY
 * - Multiple items: each is processed; paths are space-separated on a single line
 *
 * The return value is true if anything was pasted, false otherwise.
 */
export async function pasteClipboardToPty(ptyId: string, opts: PasteOptions = {}): Promise<boolean> {
  try {
    const items = await navigator.clipboard.read();
    const pathsToWrite: string[] = [];
    let pastedText: string | null = null;

    for (const item of items) {
      // 1. Prefer text if present (but keep iterating — we allow mixed)
      if (!opts.skipText && item.types.includes("text/plain") && pastedText === null) {
        try {
          const blob = await item.getType("text/plain");
          const text = await blob.text();
          if (text) pastedText = text;
          continue;
        } catch { /* fall through */ }
      }

      // 2. Image types
      const imageType = item.types.find((t) => t.startsWith("image/"));
      if (imageType) {
        try {
          const blob = await item.getType(imageType);
          const ext = extFromMime(imageType);
          const arrayBuf = await blob.arrayBuffer();
          const bytes = Array.from(new Uint8Array(arrayBuf));
          const path = await invoke<string>("save_clipboard_image", { bytes, ext });
          if (path) pathsToWrite.push(path);
          continue;
        } catch { /* fall through */ }
      }

      // 3. Other MIME types — treat as generic files
      const otherType = item.types.find((t) => t !== "text/plain");
      if (otherType) {
        try {
          const blob = await item.getType(otherType);
          const filename = filenameFromMime(otherType);
          const arrayBuf = await blob.arrayBuffer();
          const bytes = Array.from(new Uint8Array(arrayBuf));
          const path = await invoke<string>("save_clipboard_file", { bytes, filename });
          if (path) pathsToWrite.push(path);
        } catch { /* ignore */ }
      }
    }

    // Write text first (if any), then file paths joined by space
    if (pastedText !== null) {
      await invoke("write_pty_string", { ptyId, data: pastedText });
    }
    if (pathsToWrite.length > 0) {
      const joined = pathsToWrite.join(" ");
      await invoke("write_pty_string", { ptyId, data: joined });
    }

    return pastedText !== null || pathsToWrite.length > 0;
  } catch {
    // Fallback: text-only via readText()
    try {
      const text = await navigator.clipboard.readText();
      if (text) {
        await invoke("write_pty_string", { ptyId, data: text });
        return true;
      }
    } catch { /* ignore */ }
    return false;
  }
}

/** Extract file extension from MIME type, defaulting to "bin". */
function extFromMime(mime: string): string {
  const map: Record<string, string> = {
    "image/png": "png",
    "image/jpeg": "jpg",
    "image/gif": "gif",
    "image/webp": "webp",
    "image/bmp": "bmp",
    "image/svg+xml": "svg",
  };
  if (map[mime]) return map[mime];
  const slash = mime.indexOf("/");
  return slash >= 0 ? mime.slice(slash + 1).split(";")[0] : "bin";
}

/** Generate a reasonable filename for an unknown MIME type. */
function filenameFromMime(mime: string): string {
  const ext = extFromMime(mime);
  return `clipboard.${ext}`;
}
