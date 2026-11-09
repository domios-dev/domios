import { computeFitTransform, type ViewportBox, type ViewportTransform } from "@codeterm/canvas-camera";

const CAMERA_LIMIT = 24;
const cameras = new Map<string, ViewportTransform>();

export type InitialCanvasFit = "pending" | "ready" | "cancelled";

export function imageCanvasKey(images: readonly { src: string; path?: string }[]): string {
  let hash = 2166136261;
  let second = 5381;
  for (const image of images) {
    const source = image.path || image.src;
    for (let index = 0; index < source.length; index++) {
      hash = Math.imul(hash ^ source.charCodeAt(index), 16777619);
      second = Math.imul(second, 33) ^ source.charCodeAt(index);
    }
    hash = Math.imul(hash ^ source.length, 16777619);
    second = Math.imul(second, 33) ^ source.length;
  }
  return `${images.length}:${hash >>> 0}:${second >>> 0}`;
}

export function createImageCanvasCamera(key: string, memory = cameras) {
  let initialized = false;
  const remember = (transform: ViewportTransform) => {
    if (!initialized) return;
    memory.delete(key);
    memory.set(key, { ...transform });
    if (memory.size > CAMERA_LIMIT) {
      const oldest = memory.keys().next().value;
      if (oldest !== undefined) memory.delete(oldest);
    }
  };
  const fit = (bounds: ViewportBox, width: number, height: number) =>
    computeFitTransform(bounds, width, height, { minScale: 0.0001, maxScale: 8 });
  return {
    restored: memory.has(key),
    remember,
    fit,
    takeControl(transform: ViewportTransform) {
      initialized = true;
      remember(transform);
    },
    initialize(bounds: ViewportBox, width: number, height: number, measured = true) {
      if (initialized || width <= 0 || height <= 0) return null;
      const transform = memory.get(key) ?? (measured ? fit(bounds, width, height) : null);
      if (transform) initialized = true;
      return transform;
    },
  };
}
