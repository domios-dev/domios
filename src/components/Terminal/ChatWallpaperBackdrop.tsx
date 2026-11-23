import { createContext, useContext, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import type { WallpaperAsset } from "../../lib/useMachineAppearance";

interface Layer {
  key: number;
  style: CSSProperties;
  enter: boolean;
  asset?: WallpaperAsset;
}

interface Backdrop {
  layers: Layer[];
  settled: (key: number) => void;
  switching: boolean;
  finishSwitch: (key: number) => void;
}

export const ChatWallpaperContext = createContext<Backdrop | null>(null);

interface Display {
  scope: string;
  latest: Layer;
  previous: Layer[];
}

export function useChatWallpaperBackdrop(scope: string, ready: boolean, style: CSSProperties, asset?: WallpaperAsset): Backdrop {
  const [display, setDisplay] = useState<Display>({
    scope, latest: { key: 0, style, enter: false, asset }, previous: [],
  });
  if (ready && display.scope !== scope) {
    setDisplay({
      scope,
      latest: { key: display.latest.key + 1, style, enter: true, asset },
      previous: [...display.previous, display.latest],
    });
  } else if (ready && display.latest.style !== style) {
    setDisplay({ ...display, latest: { ...display.latest, style, asset } });
  }
  const leased = useRef(new Map<WallpaperAsset, () => void>());
  useLayoutEffect(() => {
    const active = new Set([display.latest.asset, ...display.previous.map(layer => layer.asset)]);
    for (const asset of active) {
      if (asset && !leased.current.has(asset)) leased.current.set(asset, asset.retain());
    }
    for (const [asset, release] of leased.current) {
      if (!active.has(asset)) { release(); leased.current.delete(asset); }
    }
  }, [display]);
  useLayoutEffect(() => () => {
    for (const release of leased.current.values()) release();
    leased.current.clear();
  }, []);
  return {
    layers: [...display.previous, display.latest],
    switching: display.latest.enter,
    settled(key) {
      setDisplay(current => current.latest.key === key && current.previous.length
        ? { ...current, previous: [] } : current);
    },
    finishSwitch(key) {
      setDisplay(current => current.latest.key === key
        ? { ...current, latest: { ...current.latest, enter: false }, previous: [] } : current);
    },
  };
}

export function ChatWallpaperBackdrop() {
  const backdrop = useContext(ChatWallpaperContext);
  if (!backdrop) return <div aria-hidden="true" className="ct-chat-wallpaper" />;
  return <div aria-hidden="true" className="ct-chat-wallpaper-stack">
    {backdrop.layers.map((layer, index) => <div key={layer.key} className="ct-chat-wallpaper"
      data-enter={layer.enter || undefined}
      data-frozen={index < backdrop.layers.length - 1 || undefined}
      style={layer.style}
      onAnimationEnd={event => {
        if (event.target === event.currentTarget) backdrop.settled(layer.key);
      }}
    />)}
  </div>;
}
