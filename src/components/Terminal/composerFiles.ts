import { useState, type DragEvent } from "react";
import { attachmentKindForPath } from "@codeterm/chat-core";
import { invoke } from "../../lib/bridge";
import { IS_WEB } from "../../lib/webApi";
import type { InlineAttachment } from "./useComposerCapsules";
import { storeBlob } from "./voice/audio";
import { isFileTransfer, planFileDrop } from "./composerTransferModel";

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
export async function classifyAttachmentPaths(paths: string[]): Promise<InlineAttachment[]> {
  let kinds: string[] = [];
  try {
    kinds = await invoke<string[]>("attachment_path_kinds", { paths });
  } catch {
    kinds = [];
  }
  return paths.map((path, index) => ({ path, kind: kinds[index] === "folder" ? "folder" : attachmentKindForPath(path) }));
}

export function useWebFileDrop(
  onAttach: (paths: string[]) => void,
  onError: (msg: string) => void,
  uploadFiles?: (files: File[]) => Promise<void>,
) {
  const [dragOver, setDragOver] = useState(false);
  if (!IS_WEB) {
    return { dragOver: false, handlers: {} };
  }
  const handlers = {
    onDragOver: (e: DragEvent) => {
      if (isFileTransfer(e.dataTransfer)) {
        e.preventDefault();
        setDragOver(true);
      }
    },
    onDragLeave: (e: DragEvent) => {
      if (
        e.currentTarget === e.target ||
        !e.currentTarget.contains(e.relatedTarget as Node)
      ) {
        setDragOver(false);
      }
    },
    onDrop: (e: DragEvent) => {
      setDragOver(false);
      const { preventNavigation, files, error } = planFileDrop(e.dataTransfer);
      if (preventNavigation) e.preventDefault();
      if (error) onError(error);
      if (!files.length) return;
      void (uploadFiles ? uploadFiles(files) : storeBrowserFiles(files, onAttach, onError)).catch(error => onError(error instanceof Error ? error.message : String(error)));
    },
  };
  return { dragOver, handlers };
}
