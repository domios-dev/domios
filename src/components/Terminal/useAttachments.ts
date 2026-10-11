import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, IS_WEB } from "../../lib/bridge";
import { useTauriFileDrop, materializeDroppedPaths } from "../../hooks/terminal";
import { classifyAttachmentPaths, useWebFileDrop } from "./composerFiles";
import type { InlineAttachment, InlineAttachmentSink } from "./useComposerCapsules";
import { useAttachmentUploads } from "./useAttachmentUploads";
import { needsAsyncClipboardImageRead } from "./composerAttachmentPlan";

const IMAGE_EXT: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/bmp": "bmp",
  "image/svg+xml": "svg",
};

const handledPastes = new WeakSet<ClipboardEvent>();

function imageExt(mime: string): string {
  return IMAGE_EXT[mime] ?? (mime.split("/")[1]?.split(";")[0] || "png");
}

// Native WebViews can omit screenshot data from the paste event.
async function readClipboardImage(syncFile: File | null): Promise<{ blob: Blob; ext: string } | null> {
  if (syncFile) return { blob: syncFile, ext: imageExt(syncFile.type) };
  try {
    if (navigator.clipboard && "read" in navigator.clipboard) {
      const items = await navigator.clipboard.read();
      for (const item of items) {
        const type = item.types.find((t) => t.startsWith("image/"));
        if (type) return { blob: await item.getType(type), ext: imageExt(type) };
      }
    }
  } catch {
    // Unsupported native clipboard readers leave the draft untouched.
  }
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

  const inlineSinkRef = useRef<InlineAttachmentSink | null>(null);
  const deliver = useCallback((items: InlineAttachment[]) => {
    if (items.length === 0) return;
    const sink = inlineSinkRef.current;
    if (sink) sink(items);
    else addAttachments(items.map((item) => item.path));
  }, [addAttachments]);
  const deliverPaths = useCallback(async (paths: string[]) => {
    const clean = paths.map((p) => p.trim()).filter((p, i, all) => p.length > 0 && all.indexOf(p) === i);
    if (clean.length) deliver(await classifyAttachmentPaths(clean));
  }, [deliver]);

  const uploads = useAttachmentUploads(deliverPaths);

  const handlePasteEvent = useCallback(async (e: ClipboardEvent) => {
    if (handledPastes.has(e)) return;
    handledPastes.add(e);
    const cd = e.clipboardData;
    const files = Array.from(cd?.files ?? []);
    if (!files.length) {
      files.push(...Array.from(cd?.items ?? [])
        .filter((item) => item.kind === "file")
        .map((item) => item.getAsFile())
        .filter((file): file is File => file !== null));
    }
    if (IS_WEB) {
      if (files.length) {
        e.preventDefault();
        e.stopImmediatePropagation();
        await uploads.addFiles(files);
      }
      return;
    }
    const syncImageFile = files.find((file) => file.type.startsWith("image/")) ?? null;
    if (syncImageFile) {
      e.preventDefault();
      e.stopImmediatePropagation();
    }

    // 1. Native OS file paths (Explorer/Finder copy) — Tauri only; throws in web.
    try {
      const paths = await invoke<string[]>("read_clipboard_files");
      if (paths && paths.length > 0) {
        e.preventDefault();
        await deliverPaths(await materializeDroppedPaths(paths));
        return;
      }
    } catch {
      // Not in Tauri, or no files on the clipboard — continue.
    }

    const image = needsAsyncClipboardImageRead(Array.from(cd?.types ?? []), syncImageFile !== null)
      ? await readClipboardImage(null)
      : syncImageFile && { blob: syncImageFile, ext: imageExt(syncImageFile.type) };
    if (image) {
      e.preventDefault();
      try {
        await uploads.add(image.blob, `pasted-image-${Date.now()}.${image.ext}`);
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      }
    }
  }, [deliverPaths, onError, uploads.addFiles, uploads.add]);

  useEffect(() => {
    const onWindowPaste = (e: ClipboardEvent) => {
      if (
        e.defaultPrevented ||
        (chatRootRef.current &&
        !chatRootRef.current.contains(document.activeElement))
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
    onPaths: (paths) => void deliverPaths(paths),
  });

  const { dragOver: webDragOver, handlers: webDropHandlers } = useWebFileDrop(
    (paths) => void deliverPaths(paths),
    onError,
    uploads.addFiles,
  );

  return {
    uploadStatus: uploads.status,
    attachments,
    addAttachments,
    removeAttachment,
    clearAttachments,
    handlePasteEvent,
    inlineSinkRef,
    webDragOver,
    webDropHandlers,
  };
}
