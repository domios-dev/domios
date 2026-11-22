import { useState, type DragEvent } from "react";
import { attachmentKindForPath } from "@codeterm/chat-core";
import { invoke } from "../../lib/bridge";
import { IS_WEB } from "../../lib/webApi";
import type { InlineAttachment } from "./useComposerCapsules";
import { storeBlob } from "./voice/audio";
import { acceptsFileDropPreview, identifyOpaqueImage, isFileTransfer, planFileDrop, unreadableFileDrop } from "./composerTransferModel";

export async function storeBrowserFiles(
  files: Iterable<File>,
  onAttach: (paths: string[]) => void,
  onError: (msg: string) => void,
): Promise<void> {
  const paths: string[] = [];
  for (const f of files) {
    try {
      const path = await storeBlob(f, f.name || "file.bin");
      if (path.trim()) paths.push(path);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  }
  if (paths.length > 0) onAttach(paths);
}

/** Folders become folder capsules; the host answers where it can stat the path, the extension decides elsewhere. */
export async function classifyAttachmentPaths(paths: string[], originalPaths?: string[]): Promise<InlineAttachment[]> {
  let kinds: string[] = [];
  try {
    kinds = await invoke<string[]>("attachment_path_kinds", { paths });
  } catch {
    kinds = [];
  }
  return paths.map((path, index) => ({ path, kind: kinds[index] === "folder" ? "folder" : attachmentKindForPath(path),
    ...(originalPaths?.[index] ? { displayName: originalPaths[index].split(/[\\/]/).pop() } : {}) }));
}

export function useWebFileDrop(
  onAttach: (paths: string[]) => void,
  onError: (msg: string) => void,
  uploadFiles?: (files: File[]) => Promise<void>,
) {
  const [dragOver, setDragOver] = useState(false);
  const handlers = {
    onDragOverCapture: (e: DragEvent) => {
      const files = isFileTransfer(e.dataTransfer);
      if (files || (IS_WEB && acceptsFileDropPreview(e.dataTransfer))) e.preventDefault();
      if (files) setDragOver(true);
    },
    onDragLeaveCapture: (e: DragEvent) => {
      if (
        e.currentTarget === e.target ||
        !e.currentTarget.contains(e.relatedTarget as Node)
      ) {
        setDragOver(false);
      }
    },
    onDropCapture: (e: DragEvent) => {
      setDragOver(false);
      if (!IS_WEB) {
        if (isFileTransfer(e.dataTransfer)) e.preventDefault();
        return;
      }
      e.preventDefault();
      e.stopPropagation();
      const { files, needsImageType, error } = planFileDrop(e.dataTransfer);
      if (error) onError(error);
      if (!files.length) return;
      const attach = async () => {
        if (needsImageType) {
          const image = await identifyOpaqueImage(files[0]);
          if (!image) { onError(unreadableFileDrop()); return; }
          files[0] = image;
        }
        await (uploadFiles ? uploadFiles(files) : storeBrowserFiles(files, onAttach, onError));
      };
      void attach().catch(error => onError(error instanceof Error ? error.message : String(error)));
    },
  };
  return { dragOver: dragOver && IS_WEB, handlers };
}
