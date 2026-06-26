const FLAG_KEY = "canvas.passiveRenderer";

let cached: boolean | null = null;

export function passiveRendererEnabled(): boolean {
  if (cached !== null) return cached;
  if (typeof window === "undefined") {
    cached = false;
    return cached;
  }
  try {
    cached = window.localStorage.getItem(FLAG_KEY) === "true";
  } catch {
    cached = false;
  }
  return cached;
}

export const PASSIVE_RENDERER_FLAG_KEY = FLAG_KEY;
