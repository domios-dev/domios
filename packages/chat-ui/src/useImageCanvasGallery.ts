import { useMemo, useState } from "react";
import { galleryNext, layoutGallery } from "./galleryModel";
import type { FullscreenImage } from "./ImageFullscreen";
import { imageCanvasKey, type InitialCanvasFit } from "./imageCanvasCamera";

export function useImageCanvasGallery(image: FullscreenImage, images?: FullscreenImage[]) {
  const gallery = useMemo(() => images?.length ? images : [image], [images, image]);
  const [active, setActive] = useState(Math.max(0, gallery.indexOf(image)));
  const [initialFocusIndex] = useState(() => typeof window !== "undefined" && window.matchMedia("(max-width: 600px)").matches ? Math.max(0, gallery.indexOf(image)) : null);
  const [cameraKey] = useState(() => imageCanvasKey(gallery));
  const [initialFitCancelled, setInitialFitCancelled] = useState(false);
  const [sizes, setSizes] = useState<Record<number, { width: number; height: number }>>({});
  const layout = useMemo(() => layoutGallery(gallery.map((_, index) => sizes[index] ?? { width: 800, height: 600 })), [gallery, sizes]);
  const focus = initialFocusIndex === null ? null : { request: 0, bounds: layout.items[initialFocusIndex] ?? layout.bounds };
  const measured = gallery.every((item, index) => item.error || !item.pending && sizes[index] !== undefined);
  const initialFit: InitialCanvasFit = initialFitCancelled ? "cancelled" : measured ? "ready" : "pending";
  const select = (index: number) => {
    setInitialFitCancelled(true);
    setActive(index);
  };
  const measure = (index: number, width: number, height: number) => {
    if (width && height) setSizes(previous => previous[index]?.width === width && previous[index]?.height === height ? previous : { ...previous, [index]: { width, height } });
  };
  return { gallery, active, selected: gallery[Math.min(active, gallery.length - 1)], layout, cameraKey, initialFit, focus, select, measure, navigate: (direction: number) => select(galleryNext(active, direction, gallery.length)) };
}
