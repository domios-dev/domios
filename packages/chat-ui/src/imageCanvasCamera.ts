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
