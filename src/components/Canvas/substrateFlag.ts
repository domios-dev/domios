const FLAG_KEY = "canvas.substrateV2";

let cached: boolean | null = null;

export function substrateV2Enabled(): boolean {
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

export function __resetSubstrateV2ForTest(): void {
  cached = null;
}
