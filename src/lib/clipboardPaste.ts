import { invoke } from "@tauri-apps/api/core";

export interface PasteOptions {
  /** Skip text items — on macOS Cmd+V native handling writes text itself. */
  skipText?: boolean;
}

export function looksLikeAbsolutePath(s: string): boolean {
  const t = s.trim();
  return /^([A-Za-z]:[\\/]|\/|~\/)/.test(t) && !t.includes("\n") && t.length < 1000;
}

export function fileUriToPath(uri: string): string | null {
  if (!uri.startsWith("file://")) return null;
  let raw = uri.slice("file://".length);
  if (raw.startsWith("/")) {
    const afterSlash = raw.slice(1);
    if (/^[A-Za-z]:[\\/]/.test(afterSlash)) raw = afterSlash;
  }
  try {
    return decodeURI(raw);
  } catch {
    return raw;
  }
}

// Extracts real file paths the OS attached to the clipboard alongside a
// possibly-bogus image preview. Without this, macOS Cmd+C of a Finder file
// followed by Cmd+V gives a 1024x1024 placeholder PNG via getAsFile().
export function extractFilePathsFromClipboard(cd: DataTransfer): string[] {
  const out: string[] = [];
  const seen = new Set<string>();

  const uriList = cd.getData("text/uri-list");
  if (uriList) {
    for (const line of uriList.split(/\r?\n/)) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#")) continue;
      const p = fileUriToPath(trimmed) ?? (looksLikeAbsolutePath(trimmed) ? trimmed.trim() : null);
      if (p && !seen.has(p)) { seen.add(p); out.push(p); }
    }
  }

  if (out.length === 0) {
    const plain = cd.getData("text/plain");
    if (plain && looksLikeAbsolutePath(plain)) {
      const p = plain.trim();
      if (!seen.has(p)) { seen.add(p); out.push(p); }
    }
  }

  return out;
}

export function quotePathForPaste(p: string): string {
  if (!/[\s"'$`!<>|&;()*?#~]/.test(p)) return p;
  if (!p.includes("'")) return `'${p}'`;
  return `"${p.replace(/(["\\$`])/g, "\\$1")}"`;
}

export async function sendFilePathsToPty(ptyId: string, paths: string[]): Promise<boolean> {
  if (paths.length === 0) return false;
  const data = paths.map(quotePathForPaste).join(" ");
  await invoke("write_pty_string", { ptyId, data });
  return true;
}

/** Read clipboard via web API + native file-ref fallback, route to PTY. */
export async function pasteClipboardToPty(ptyId: string, opts: PasteOptions = {}): Promise<boolean> {
  const skipText = opts.skipText ?? false;
  const nativePaths = await readNativeClipboardFiles();
  if (nativePaths.length > 0) {
    return sendFilePathsToPty(ptyId, nativePaths);
  }

  // skipText: xterm.onData already delivers text from a Cmd+V / Ctrl+V; only
  // images and saved files need our handling.
  const web = await readWebClipboard(skipText);
  if (web.text !== null) {
    await invoke("write_pty_string", { ptyId, data: web.text });
  }
  if (web.paths.length > 0) {
    await sendFilePathsToPty(ptyId, web.paths);
  }
  if (web.text !== null || web.paths.length > 0) return true;

  if (skipText) return false;
  return fallbackReadText(ptyId);
}

async function readNativeClipboardFiles(): Promise<string[]> {
  try {
    const paths = await invoke<string[]>("read_clipboard_files");
    return paths || [];
  } catch {
    return [];
  }
}

interface WebClipboardResult {
  paths: string[];
  text: string | null;
}

async function readWebClipboard(skipText: boolean): Promise<WebClipboardResult> {
  try {
    const items = await navigator.clipboard.read();
    const paths: string[] = [];
    let text: string | null = null;

    for (const item of items) {
      const imagePath = await saveImageItem(item);
      if (imagePath) { paths.push(imagePath); continue; }

      const filePath = await saveFileItem(item);
      if (filePath) { paths.push(filePath); continue; }

      if (!skipText && text === null) {
        text = await readTextItem(item);
      }
    }

    return { paths, text };
  } catch {
    return { paths: [], text: null };
  }
}

async function saveImageItem(item: ClipboardItem): Promise<string | null> {
  const imageType = item.types.find((t) => t.startsWith("image/"));
  if (!imageType) return null;
  try {
    const bytes = await blobBytes(await item.getType(imageType));
    const ext = extFromMime(imageType);
    return await invoke<string>("save_clipboard_image", { bytes, ext });
  } catch {
    return null;
  }
}

async function saveFileItem(item: ClipboardItem): Promise<string | null> {
  const mimeType = item.types.find((t) => t !== "text/plain" && !t.startsWith("image/"));
  if (!mimeType) return null;
  try {
    const bytes = await blobBytes(await item.getType(mimeType));
    const filename = `clipboard.${extFromMime(mimeType)}`;
    return await invoke<string>("save_clipboard_file", { bytes, filename });
  } catch {
    return null;
  }
}

async function readTextItem(item: ClipboardItem): Promise<string | null> {
  if (!item.types.includes("text/plain")) return null;
  try {
    const blob = await item.getType("text/plain");
    const text = await blob.text();
    return text || null;
  } catch {
    return null;
  }
}

async function blobBytes(blob: Blob): Promise<number[]> {
  return Array.from(new Uint8Array(await blob.arrayBuffer()));
}

async function fallbackReadText(ptyId: string): Promise<boolean> {
  try {
    const text = await navigator.clipboard.readText();
    if (!text) return false;
    await invoke("write_pty_string", { ptyId, data: text });
    return true;
  } catch {
    return false;
  }
}

const MIME_EXT: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/bmp": "bmp",
  "image/svg+xml": "svg",
};

function extFromMime(mime: string): string {
  if (MIME_EXT[mime]) return MIME_EXT[mime];
  const slash = mime.indexOf("/");
  return slash >= 0 ? mime.slice(slash + 1).split(";")[0] : "bin";
}
