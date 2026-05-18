import { invoke } from "@tauri-apps/api/core";

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
 * state. Reads the OS clipboard directly (Tauri files + navigator.clipboard
 * APIs) — does NOT depend on `event.clipboardData`, which WebView2 fills
 * unreliably for keyboard-triggered pastes on Windows. */
export async function routeClipboardToTerm(term: TermLike): Promise<boolean> {
  const filePaths = await readNativeClipboardFiles();
  if (filePaths.length > 0) {
    term.paste(filePaths.map(quotePathForPaste).join(" "));
    return true;
  }

  // readText() before read()+classifier: WebView2 truncates long blobs
  // returned by ClipboardItem.getType("text/plain"); readText reliably
  // delivers the full string.
  const plain = await readPlainTextSafely();
  if (plain) {
    term.paste(plain);
    return true;
  }

  // No plain text — classifier path: images get saved to a temp file and
  // pasted as a shell-quoted path; HTML-only is handled the same way.
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
