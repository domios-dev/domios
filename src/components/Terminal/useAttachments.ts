import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import { useTauriFileDrop } from "../../hooks/terminal";
import { useWebFileDrop } from "./ComposerActions";
import { storeBlob } from "./voice/audio";

const IMAGE_EXT: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/bmp": "bmp",
  "image/svg+xml": "svg",
};

function imageExt(mime: string): string {
  return IMAGE_EXT[mime] ?? (mime.split("/")[1]?.split(";")[0] || "png");
}

/**
 * Read a pasted image as a Blob. The synchronous paste event omits clipboard
 * image DATA (screenshots) on WebView2 (Windows) / WKWebView (macOS), so prefer
 * the async Clipboard API — which the terminal already uses successfully — and
 * fall back to the sync File captured during the event dispatch (#152).
 */
async function readClipboardImage(syncFile: File | null): Promise<{ blob: Blob; ext: string } | null> {
  try {
    if (navigator.clipboard && "read" in navigator.clipboard) {
      const items = await navigator.clipboard.read();
      for (const item of items) {
        const type = item.types.find((t) => t.startsWith("image/"));
        if (type) return { blob: await item.getType(type), ext: imageExt(type) };
      }
    }
  } catch {
    // Permission denied / unsupported — fall back to the sync file.
  }
  if (syncFile) return { blob: syncFile, ext: imageExt(syncFile.type) };
  return null;
}

export function useAttachments({
  chatRootRef,
  enabled,
  onError,
}: {
  chatRootRef: React.RefObject<HTMLDivElement | null>;
  enabled: boolean;
  onError: (message: string) => void;
}) {
  const [attachments, setAttachments] = useState<string[]>([]);

  const addAttachments = useCallback((paths: string[]) => {
    const cleanPaths = paths
      .map((p) => p.trim())
      .filter((p, i, all) => p.length > 0 && all.indexOf(p) === i);
    if (cleanPaths.length === 0) return;
    setAttachments((prev) => {
      const seen = new Set(prev);
      const next = [...prev];
      for (const p of cleanPaths) {
        if (!seen.has(p)) {
          seen.add(p);
          next.push(p);
        }
      }
      return next;
    });
  }, []);

  const removeAttachment = useCallback((path: string) => {
    setAttachments((prev) => prev.filter((x) => x !== path));
  }, []);

  const clearAttachments = useCallback(() => setAttachments([]), []);

  const handlePasteEvent = useCallback(async (e: ClipboardEvent) => {
    const cd = e.clipboardData;
    // Capture the sync image File now — clipboardData is only valid during the
    // synchronous event dispatch; the async reads below would see it emptied.
    const syncImageFile = cd
      ? (Array.from(cd.items).find((it) => it.kind === "file" && it.type.startsWith("image/"))?.getAsFile() ?? null)
      : null;
    // When the image is visible synchronously, stop the default paste AND the
    // terminal's own window paste handler now (both can't be cancelled after an
    // await) so the screenshot attaches here instead of leaking to the PTY.
    if (syncImageFile) {
      e.preventDefault();
      e.stopImmediatePropagation();
    }

    // 1. Native OS file paths (Explorer/Finder copy) — Tauri only; throws in web.
    try {
      const paths = await invoke<string[]>("read_clipboard_files");
      if (paths && paths.length > 0) {
        e.preventDefault();
        addAttachments(paths);
        return;
      }
    } catch {
      // Not in Tauri, or no files on the clipboard — continue.
    }

    // 2. Screenshot / image. The synchronous paste event omits clipboard image
    //    DATA on WebView2 / WKWebView, so a pasted screenshot never attached and
    //    leaked to the terminal (#152). Read it via the async Clipboard API and
    //    save through storeBlob (→ /api/upload in web, temp file in Tauri).
    const image = await readClipboardImage(syncImageFile);
    if (image) {
      e.preventDefault();
      try {
        const path = await storeBlob(image.blob, `pasted-image-${Date.now()}.${image.ext}`);
        if (path.trim()) addAttachments([path]);
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      }
    }
  }, [addAttachments, onError]);

  useEffect(() => {
    const onWindowPaste = (e: ClipboardEvent) => {
      if (
        chatRootRef.current &&
        !chatRootRef.current.contains(document.activeElement)
      ) {
        return;
      }
      void handlePasteEvent(e);
    };
    window.addEventListener("paste", onWindowPaste);
    return () => window.removeEventListener("paste", onWindowPaste);
  }, [chatRootRef, handlePasteEvent]);

  useTauriFileDrop({
    containerRef: chatRootRef as React.RefObject<HTMLElement | null>,
    enabled,
    onPaths: (paths) => addAttachments(paths),
  });

  const { dragOver: webDragOver, handlers: webDropHandlers } = useWebFileDrop(
    addAttachments,
    onError,
  );

  return {
    attachments,
    addAttachments,
    removeAttachment,
    clearAttachments,
    handlePasteEvent,
    webDragOver,
    webDropHandlers,
  };
}
