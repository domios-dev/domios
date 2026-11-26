import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useState, type CSSProperties } from "react";
import type { WallpaperAsset } from "../../lib/useMachineAppearance";
import {
  applyRasterizedWallpaper,
  getCachedRasterTile,
  planWallpaperRaster,
  rasterizeWallpaperTile,
} from "../../lib/wallpaperTileCache";

export const ChatWallpaperContext = createContext<CSSProperties | undefined>(undefined);

export function useChatWallpaperAssetLease(asset?: WallpaperAsset) {
  useLayoutEffect(() => asset?.retain(), [asset]);
}

export function ChatWallpaperBackdrop({ active = true }: { active?: boolean }) {
  const style = useContext(ChatWallpaperContext);
  const plan = useMemo(() => (active ? planWallpaperRaster(style) : null), [active, style]);
  const dpr = typeof window !== "undefined" && Number.isFinite(window.devicePixelRatio) ? window.devicePixelRatio : 1;
  const cachedBlob = plan ? getCachedRasterTile(plan, dpr) : null;
  const [raster, setRaster] = useState<{ plan: typeof plan; blobUrl: string } | null>(null);

  useEffect(() => {
    if (!active || !plan || cachedBlob) return;
    let alive = true;
    void rasterizeWallpaperTile(plan, dpr).then((blobUrl) => {
      if (alive && blobUrl) setRaster({ plan, blobUrl });
    });
    return () => { alive = false; };
  }, [active, plan, cachedBlob, dpr]);

  if (!active) return null;
  const blobUrl = cachedBlob ?? (raster?.plan === plan ? raster.blobUrl : null);
  const effectiveStyle = style && plan && blobUrl ? applyRasterizedWallpaper(style, plan, blobUrl) : style;
  return <div aria-hidden="true" className="ct-chat-wallpaper-stack">
    <div className="ct-chat-wallpaper" style={effectiveStyle} />
  </div>;
}
