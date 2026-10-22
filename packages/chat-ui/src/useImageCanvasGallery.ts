import { useMemo, useState } from "react";
import { galleryNext, layoutGallery } from "./galleryModel";
import type { FullscreenImage } from "./ImageFullscreen";

export function useImageCanvasGallery(image: FullscreenImage, images?: FullscreenImage[]) {
  const gallery = useMemo(() => images?.length ? images : [image], [images, image]);
  const [active, setActive] = useState(Math.max(0, gallery.indexOf(image)));
  const [focused, setFocused] = useState<{ index: number; request: number } | null>(() =>
    typeof window !== "undefined" && window.matchMedia("(max-width: 600px)").matches
      ? { index: Math.max(0, gallery.indexOf(image)), request: 1 } : null);
  const [sizes, setSizes] = useState<Record<number, { width: number; height: number }>>({});
  const layout = useMemo(() => layoutGallery(gallery.map((_, index) => sizes[index] ?? { width: 800, height: 600 })), [gallery, sizes]);
  const focus = useMemo(() => focused && layout.items[focused.index] ? { request: focused.request, bounds: layout.items[focused.index] } : null, [focused, layout]);
  const select = (index: number) => {
    setActive(index);
    setFocused(previous => ({ index, request: (previous?.request ?? 0) + 1 }));
  };
  const measure = (index: number, width: number, height: number) => {
    if (width && height) setSizes(previous => previous[index]?.width === width && previous[index]?.height === height ? previous : { ...previous, [index]: { width, height } });
  };
  return { gallery, active, selected: gallery[Math.min(active, gallery.length - 1)], layout, focus, select, measure, navigate: (direction: number) => select(galleryNext(active, direction, gallery.length)) };
}
