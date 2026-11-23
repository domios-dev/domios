import { createContext, useContext, useLayoutEffect, type CSSProperties } from "react";
import type { WallpaperAsset } from "../../lib/useMachineAppearance";

export const ChatWallpaperContext = createContext<CSSProperties | undefined>(undefined);

export function useChatWallpaperAssetLease(asset?: WallpaperAsset) {
  useLayoutEffect(() => asset?.retain(), [asset]);
}

export function ChatWallpaperBackdrop() {
  const style = useContext(ChatWallpaperContext);
  return <div aria-hidden="true" className="ct-chat-wallpaper-stack">
    <div className="ct-chat-wallpaper" style={style} />
  </div>;
}
