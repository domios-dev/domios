import { useState, type DragEvent } from "react";
import { attachmentKindForPath } from "@codeterm/chat-core";
import { invoke } from "../../lib/bridge";
import { IS_WEB } from "../../lib/webApi";
import type { InlineAttachment } from "./useComposerCapsules";
import { storeBlob } from "./voice/audio";

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
      if (e.dataTransfer.types.includes("Files")) {
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
      if (e.dataTransfer.files.length === 0) return;
      e.preventDefault();
      void (uploadFiles ? uploadFiles(Array.from(e.dataTransfer.files)) : storeBrowserFiles(Array.from(e.dataTransfer.files), onAttach, onError));
    },
  };
  return { dragOver, handlers };
}
