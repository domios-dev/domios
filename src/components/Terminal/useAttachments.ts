import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import { useTauriFileDrop } from "../../hooks/terminal";
import { useWebFileDrop } from "./ComposerActions";

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
    if (!cd) return;

    try {
      const paths = await invoke<string[]>("read_clipboard_files");
      if (paths && paths.length > 0) {
        e.preventDefault();
        addAttachments(paths);
        return;
      }
    } catch { /* fall through to image / text */ }

    const imageItem = Array.from(cd.items).find(
      (it) => it.kind === "file" && it.type.startsWith("image/"),
    );
    if (imageItem) {
      e.preventDefault();
      const file = imageItem.getAsFile();
      if (!file) return;
      const reader = new FileReader();
      reader.onloadend = async () => {
        const result = reader.result;
        if (typeof result !== "string") return;
        const commaIdx = result.indexOf(",");
        const b64 = commaIdx >= 0 ? result.slice(commaIdx + 1) : result;
        const mime = file.type || "image/png";
        const suffix = mime.split("/")[1] || "png";
        try {
          const path = await invoke<string>("save_temp_file", {
            data: b64,
            extension: suffix,
          });
          addAttachments([path]);
        } catch (err) {
          console.warn("[chat] paste-image failed:", err);
        }
      };
      reader.readAsDataURL(file);
    }
  }, [addAttachments]);

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
