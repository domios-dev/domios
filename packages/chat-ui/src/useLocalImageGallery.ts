import { useCallback, useEffect, useRef, useState } from "react";
import { attachmentPathName, type LocalArtifactDirective } from "@codeterm/chat-core";
import type { FullscreenImage } from "./ImageFullscreen";
import { loadLocalArtifact, type LocalPreviewFile } from "./localArtifactLoader";
import { useLocalArtifactTabId } from "./LocalArtifactScope";
import { galleryPathPlan, loadGallerySlots } from "./galleryLoading";

type Slot = FullscreenImage & { path: string };
const pendingSlot = (path: string): Slot => ({ path, name: attachmentPathName(path), src: "", pending: true });
const settled = (slot: Slot, result: { file: LocalPreviewFile } | { error: string }): Slot =>
  ({ ...slot, pending: false, src: "file" in result ? result.file.data : "", error: "error" in result ? result.error : undefined });

export function useLocalImageGallery(directive: LocalArtifactDirective, fullscreenRequest: number, onLoaded?: (file: LocalPreviewFile | null) => void) {
  const tabId = useLocalArtifactTabId();
  const plan = galleryPathPlan(directive.paths ?? [directive.path]);
  const key = JSON.stringify(plan.paths);
  const [images, setImages] = useState<Slot[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  const [reload, setReload] = useState(0);
  const anchor = useRef<HTMLElement>(null);
  const loaded = useRef(onLoaded);
  loaded.current = onLoaded;
  const current = useRef(images);
  current.current = images;
  const reloaded = useRef(`${reload}:${tabId ?? ""}`);
  // Every new path set, scope or reload starts a generation; a retry that finishes in an older one is dropped.
  const scope = useRef(0);
  useEffect(() => {
    let cancelled = false;
    scope.current += 1;
    const paths = JSON.parse(key) as string[];
    // A gallery that grows while a message streams keeps the images it already shows; only Reload reads them again.
    const generation = `${reload}:${tabId ?? ""}`;
    const keep = reloaded.current === generation ? current.current : [];
    reloaded.current = generation;
    const slots = paths.map((path, index) => keep[index]?.path === path && !keep[index].pending && !keep[index].error ? keep[index] : pendingSlot(path));
    const missing = slots.flatMap((slot, index) => slot.pending ? [index] : []);
    let delivered = slots.some(slot => !slot.pending);
    setImages(slots);
    if (!delivered) loaded.current?.(null);
    void loadGallerySlots(missing.map(index => paths[index]), path => loadLocalArtifact(path, "image", tabId), (position, result) => {
      const index = missing[position];
      setImages(previous => previous.map((slot, at) => at === index ? settled(slot, result) : slot));
      if (!delivered && "file" in result) { delivered = true; loaded.current?.(result.file); }
    }, () => cancelled);
    return () => { cancelled = true; };
  }, [key, tabId, reload]);
  useEffect(() => { if (fullscreenRequest) setOpen(0); }, [fullscreenRequest]);
  // Retrying one unavailable tile reads only that file, so its neighbours never blink.
  const retry = useCallback((index: number) => {
    const slot = current.current[index];
    if (!slot) return;
    const generation = scope.current;
    setImages(previous => previous.map((entry, at) => at === index ? pendingSlot(entry.path) : entry));
    void loadLocalArtifact(slot.path, "image", tabId).then(file => ({ file }), (error: unknown) => ({ error: error instanceof Error ? error.message : String(error) }))
      .then(result => {
        if (scope.current !== generation) return;
        setImages(previous => previous.map((entry, at) => at === index && entry.path === slot.path ? settled(entry, result) : entry));
      });
  }, [tabId]);
  return { images, open, setOpen, reload: () => setReload(value => value + 1), retry, anchor, omitted: plan.omitted, loading: images.some(image => image.pending) };
}
