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
  // First: check for native file references on the clipboard (e.g. "Copy" on a
  // JPG in Explorer puts CF_HDROP, which the web Clipboard API cannot see).
  // If present, we treat them like a drag-and-drop and skip the web API entirely.
  try {
    const filePaths = await invoke<string[]>("read_clipboard_files");
    if (filePaths && filePaths.length > 0) {
      await invoke("write_pty_string", { ptyId, data: filePaths.join(" ") });
      return true;
    }
  } catch { /* ignore — plugin may be unavailable on non-Windows */ }

  try {
    const items = await navigator.clipboard.read();
    const pathsToWrite: string[] = [];
    let pastedText: string | null = null;

    // Priority per item: image > non-text file > text.
    // An item often has multiple representations (e.g. image + text/plain with a filename).
    // We pick the richest non-text type first so binary content isn't lost.
    for (const item of items) {
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

      // Non-text MIME type → save as generic file
      const otherType = item.types.find((t) => t !== "text/plain" && !t.startsWith("image/"));
      if (otherType) {
        try {
          const blob = await item.getType(otherType);
          const filename = filenameFromMime(otherType);
          const arrayBuf = await blob.arrayBuffer();
          const bytes = Array.from(new Uint8Array(arrayBuf));
          const path = await invoke<string>("save_clipboard_file", { bytes, filename });
          if (path) pathsToWrite.push(path);
          continue;
        } catch { /* fall through */ }
      }

      // Text — take only once (first occurrence wins)
      if (!opts.skipText && item.types.includes("text/plain") && pastedText === null) {
        try {
          const blob = await item.getType("text/plain");
          const text = await blob.text();
          if (text) pastedText = text;
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

/**
 * Save an array of dropped/pasted File objects to temp via the attachments system
 * and write their paths to the PTY (space-separated, on one line).
 *
 * Used by drag-and-drop handlers to unify behavior with clipboard paste:
 * - Images → save_clipboard_image bucket
 * - All other files → save_clipboard_file bucket
 *
 * Like Telegram bridge: everything lands in tmp, only the path is typed into the terminal.
 * Returns true if at least one file was processed.
 */
export async function dropFilesToPty(ptyId: string, files: File[]): Promise<boolean> {
  if (files.length === 0) return false;
  const paths: string[] = [];

  for (const file of files) {
    try {
      const arrayBuf = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(arrayBuf));
      const name = file.name || "dropped";
      const isImage = /\.(png|jpg|jpeg|gif|webp|bmp|svg)$/i.test(name);

      let path: string;
      if (isImage) {
        const ext = (name.split(".").pop() || "png").toLowerCase();
        path = await invoke<string>("save_clipboard_image", { bytes, ext });
      } else {
        path = await invoke<string>("save_clipboard_file", { bytes, filename: name });
      }
      if (path) paths.push(path);
    } catch (err) {
      console.error("[drop] file error:", err);
    }
  }

  if (paths.length > 0) {
    await invoke("write_pty_string", { ptyId, data: paths.join(" ") });
    return true;
  }
  return false;
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
