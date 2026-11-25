import type { CSSProperties } from "react";

const MAX_CACHE_ENTRIES = 16;
const MAX_DPR = 2;
const SOLID_HEX_RE = /^#[0-9a-f]{3,8}$/i;
const SVG_TILE_RE = /^url\("([^"]+)"\)\s+0\s+0\s*\/\s*(\d+(?:\.\d+)?)px\s+\d+(?:\.\d+)?px\s+repeat(.*)$/;

export interface ParsedSvgTile {
  url: string;
  scale: number;
  suffix: string;
}

export function isSvgTileUrl(url: string): boolean {
  return url.startsWith("data:image/svg+xml,") || /\.svg(?:[?#]|$)/i.test(url);
}

export function parseSvgTileCss(cssValue: string | undefined): ParsedSvgTile | null {
  if (!cssValue) return null;
  const match = SVG_TILE_RE.exec(cssValue.trim());
  if (!match) return null;
  const url = match[1];
  const scale = Number(match[2]);
  if (!isSvgTileUrl(url) || !Number.isFinite(scale) || scale <= 0) return null;
  return { url, scale, suffix: match[3] };
}

export function resolveSolidWallpaperInk(
  ink: string | undefined,
  fgFallback: string | undefined,
): string | undefined {
  if (!ink) return undefined;
  const trimmed = ink.trim();
  if (SOLID_HEX_RE.test(trimmed)) return trimmed;
  if (trimmed === "var(--ct-fg)" && fgFallback && SOLID_HEX_RE.test(fgFallback.trim())) {
    return fgFallback.trim();
  }
  return undefined;
}

export interface WallpaperRasterPlan {
  slot: "mask" | "fill";
  tile: ParsedSvgTile;
  tintColor?: string;
}

export type WallpaperStyleVars = CSSProperties | Record<string, string | undefined>;

export function planWallpaperRaster(style: WallpaperStyleVars | undefined): WallpaperRasterPlan | null {
  if (!style) return null;
  const vars = style as Record<string, string | undefined>;
  const maskTile = parseSvgTileCss(vars["--ct-wallpaper-mask"]);
  if (maskTile) {
    return {
      slot: "mask",
      tile: maskTile,
      tintColor: resolveSolidWallpaperInk(vars["--ct-wallpaper-ink"], vars["--ct-fg"]),
    };
  }
  const fillTile = parseSvgTileCss(vars["--ct-wallpaper-fill"]);
  if (fillTile) return { slot: "fill", tile: fillTile };
  return null;
}

export function applyRasterizedWallpaper(
  style: WallpaperStyleVars,
  plan: WallpaperRasterPlan,
  blobUrl: string,
): CSSProperties {
  const tileCss = `url("${blobUrl}") 0 0 / ${plan.tile.scale}px ${plan.tile.scale}px repeat`;
  const next: Record<string, unknown> = { ...style };
  if (plan.slot === "fill") {
    next["--ct-wallpaper-fill"] = `${tileCss}${plan.tile.suffix}`;
  } else if (plan.tintColor) {
    // Solid-color patterns skip CSS mask compositing completely by blitting a pre-tinted PNG tile.
    next["--ct-wallpaper-mask"] = "none";
    next["--ct-wallpaper-ink"] = tileCss;
  } else {
    next["--ct-wallpaper-mask"] = tileCss;
  }
  return next as CSSProperties;
}

const blobCache = new Map<string, string>();
const pendingCache = new Map<string, Promise<string | null>>();

function cacheKey(url: string, pixelSize: number, tintColor?: string): string {
  return `${pixelSize}|${tintColor ?? ""}|${url}`;
}

function storeCachedBlob(key: string, blobUrl: string): void {
  blobCache.delete(key);
  blobCache.set(key, blobUrl);
  while (blobCache.size > MAX_CACHE_ENTRIES) {
    const oldest = blobCache.keys().next().value;
    if (oldest === undefined) break;
    const oldUrl = blobCache.get(oldest);
    blobCache.delete(oldest);
    if (oldUrl && typeof URL !== "undefined" && typeof URL.revokeObjectURL === "function") {
      URL.revokeObjectURL(oldUrl);
    }
  }
}

export function getCachedRasterTile(plan: WallpaperRasterPlan, dpr: number): string | null {
  const pixelSize = Math.max(1, Math.round(plan.tile.scale * Math.min(Math.max(dpr, 1), MAX_DPR)));
  return blobCache.get(cacheKey(plan.tile.url, pixelSize, plan.tintColor)) ?? null;
}

export function rasterizeWallpaperTile(plan: WallpaperRasterPlan, dpr: number): Promise<string | null> {
  if (
    typeof document === "undefined"
    || typeof Image === "undefined"
    || typeof URL === "undefined"
    || typeof URL.createObjectURL !== "function"
  ) {
    return Promise.resolve(null);
  }
  const pixelSize = Math.max(1, Math.round(plan.tile.scale * Math.min(Math.max(dpr, 1), MAX_DPR)));
  const key = cacheKey(plan.tile.url, pixelSize, plan.tintColor);
  const hit = blobCache.get(key);
  if (hit) return Promise.resolve(hit);
  const inflight = pendingCache.get(key);
  if (inflight) return inflight;

  const task = new Promise<string | null>((resolve) => {
    const img = new Image();
    img.decoding = "async";
    img.onload = () => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = pixelSize;
        canvas.height = pixelSize;
        const ctx = canvas.getContext("2d");
        if (!ctx || typeof canvas.toBlob !== "function") {
          resolve(null);
          return;
        }
        ctx.drawImage(img, 0, 0, pixelSize, pixelSize);
        if (plan.tintColor) {
          ctx.globalCompositeOperation = "source-in";
          ctx.fillStyle = plan.tintColor;
          ctx.fillRect(0, 0, pixelSize, pixelSize);
        }
        canvas.toBlob((blob) => {
          if (!blob) {
            resolve(null);
            return;
          }
          const url = URL.createObjectURL(blob);
          storeCachedBlob(key, url);
          resolve(url);
        }, "image/png");
      } catch {
        resolve(null);
      }
    };
    img.onerror = () => resolve(null);
    img.src = plan.tile.url;
  }).finally(() => {
    pendingCache.delete(key);
  });

  pendingCache.set(key, task);
  return task;
}
