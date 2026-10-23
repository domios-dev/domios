import { useEffect, useRef, useState } from "react";
import { attachmentPathName, type LocalArtifactDirective } from "@codeterm/chat-core";
import type { FullscreenImage } from "./ImageFullscreen";
import { loadLocalArtifact, type LocalPreviewFile } from "./localArtifactLoader";
import { useLocalArtifactTabId } from "./LocalArtifactScope";
import { galleryPathPlan, loadGallerySlots } from "./galleryLoading";

export function useLocalImageGallery(directive: LocalArtifactDirective, fullscreenRequest: number, onLoaded?: (file: LocalPreviewFile | null) => void) {
  const tabId = useLocalArtifactTabId();
  const plan = galleryPathPlan(directive.paths ?? [directive.path]);
  const key = JSON.stringify(plan.paths);
  const [images, setImages] = useState<FullscreenImage[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  const [reload, setReload] = useState(0);
  const anchor = useRef<HTMLElement>(null);
  const loaded = useRef(onLoaded);
  loaded.current = onLoaded;
  useEffect(() => {
    let cancelled = false;
    let delivered = false;
    const paths = JSON.parse(key) as string[];
    const slots = paths.map(path => ({ path, name: attachmentPathName(path), src: "", pending: true }));
    setImages(slots); loaded.current?.(null);
    void loadGallerySlots(paths, path => loadLocalArtifact(path, "image", tabId), (index, result) => {
      setImages(previous => previous.map((slot, position) => position === index
        ? { ...slot, pending: false, src: "file" in result ? result.file.data : "", error: "error" in result ? result.error : undefined } : slot));
      if (!delivered && "file" in result) { delivered = true; loaded.current?.(result.file); }
    }, () => cancelled);
    return () => { cancelled = true; };
  }, [key, tabId, reload]);
  useEffect(() => { if (fullscreenRequest) setOpen(0); }, [fullscreenRequest]);
  return { images, open, setOpen, reload: () => setReload(value => value + 1), anchor, omitted: plan.omitted, loading: images.some(image => image.pending) };
}
