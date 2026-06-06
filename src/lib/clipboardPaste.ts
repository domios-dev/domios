import { invoke } from "./bridge";

export type ClipboardPayload =
  | { kind: "image"; bytes: Blob; mime: string }
  | { kind: "plain"; text: string }
  | { kind: "html"; html: string };

// Minimal subset of ClipboardItem used by the classifier — keeps the helper
// testable without a real browser ClipboardItem.
export interface ClipboardItemLike {
  types: ReadonlyArray<string>;
  getType(type: string): Promise<Blob>;
}

// Why text/plain MUST win over text/html:
// Browsers and rich-text apps put both on the clipboard. Saving text/html
// to a temp file would shadow the plain text the user actually copied.
export async function classifyClipboardPayload(
  item: ClipboardItemLike,
): Promise<ClipboardPayload | null> {
  const imageType = item.types.find((t) => t.startsWith("image/"));
  if (imageType) {
    try {
      const bytes = await item.getType(imageType);
      return { kind: "image", bytes, mime: imageType };
    } catch {
      // Image read failed — fall through to text branches.
    }
  }

  if (item.types.includes("text/plain")) {
    try {
      const blob = await item.getType("text/plain");
      const text = await blob.text();
      if (text) return { kind: "plain", text };
    } catch {
      // text/plain failed — fall through to html.
    }
  }

  if (item.types.includes("text/html")) {
    try {
      const blob = await item.getType("text/html");
      const html = await blob.text();
      if (html) return { kind: "html", html };
    } catch {
      // ignore
    }
  }

  return null;
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

interface TermLike {
  paste(data: string): void;
}

/** Single entry point for every paste path (Ctrl+V, Cmd+V, right-click,
 * Ctrl+Shift+V, system context menu, DOM paste event). Routes through
 * xterm's `term.paste` so bracketed-paste-mode wrapping + line-ending
 * normalization happen exactly once and respect the terminal's live
 * state.
 *
 * Reads via Rust/Tauri pasteboard APIs first (NSPasteboard on macOS,
 * clipboard-win on Windows) — bypasses WebKit's `navigator.clipboard`
 * permission gate that, on macOS WebKit, returns nothing AND shows a
 * privacy toast when `readText()` is called outside a paste-event handler.
 * Only falls back to `navigator.clipboard` / `clipboardData` for
 * platforms without a native reader (Linux) or when both native sources
 * are empty. */
export async function routeClipboardToTerm(term: TermLike): Promise<boolean> {
  // 1. Native pasteboard files (Finder copy / Explorer copy)
  const filePaths = await readNativeClipboardFiles();
  if (filePaths.length > 0) {
    term.paste(filePaths.map(quotePathForPaste).join(" "));
    return true;
  }

  // 2. Native pasteboard text — works on macOS without WebKit permission
  const nativeText = await readNativeClipboardText();
  if (nativeText) {
    term.paste(nativeText);
    return true;
  }

  // 3. Web Clipboard API fallback for platforms without native readers
  //    (Linux) or edge cases where pasteboard didn't surface the text.
  const plain = await readPlainTextSafely();
  if (plain) {
    term.paste(plain);
    return true;
  }

  // 4. Image / HTML classifier — saves to a temp file and pastes the path.
  const web = await readWebClipboard();
  if (web.paths.length > 0) {
    term.paste(web.paths.map(quotePathForPaste).join(" "));
    return true;
  }
  if (web.text) {
    term.paste(web.text);
    return true;
  }

  return false;
}

async function readNativeClipboardText(): Promise<string | null> {
  try {
    const text = await invoke<string | null>("read_clipboard_text");
    return text || null;
  } catch {
    return null;
  }
}

async function readPlainTextSafely(): Promise<string | null> {
  try {
    const text = await navigator.clipboard.readText();
    return text || null;
  } catch {
    return null;
  }
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

async function readWebClipboard(): Promise<WebClipboardResult> {
  try {
    const items = await navigator.clipboard.read();
    const paths: string[] = [];
    let text: string | null = null;

    for (const item of items) {
      const payload = await classifyClipboardPayload(item);
      if (!payload) continue;

      if (payload.kind === "image") {
        const path = await saveImagePayload(payload.bytes, payload.mime);
        if (path) paths.push(path);
        continue;
      }

      if (payload.kind === "plain") {
        if (text === null) text = payload.text;
        continue;
      }

      // payload.kind === "html" — only reached when text/plain wasn't on the
      // clipboard; keeps the "save HTML to file, paste path" fallback for
      // sources that provide HTML alone.
      const path = await saveHtmlPayload(payload.html);
      if (path) paths.push(path);
    }

    return { paths, text };
  } catch {
    return { paths: [], text: null };
  }
}

async function saveImagePayload(blob: Blob, mime: string): Promise<string | null> {
  try {
    const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
    const ext = extFromMime(mime);
    return await invoke<string>("save_clipboard_image", { bytes, ext });
  } catch {
    return null;
  }
}

async function saveHtmlPayload(html: string): Promise<string | null> {
  try {
    const bytes = Array.from(new TextEncoder().encode(html));
    return await invoke<string>("save_clipboard_file", { bytes, filename: "clipboard.html" });
  } catch {
    return null;
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
