import { useContext, useId, useState, type CSSProperties } from "react";
import { FlaskConical } from "lucide-react";
import type { ChatWallpaper } from "../../../gen/ChatWallpaper";
import type { AppConfig } from "../../lib/types";
import { appearanceError } from "../../lib/appearanceClient";
import { effectiveWallpaper, wallpaperRenderStyle, wallpaperSummary } from "../../lib/chatWallpaper";
import { saveWallpaper } from "../../lib/chatWallpaperClient";
import { isLightColor } from "../../lib/themeCss";
import { MachineAppearanceContext, useWallpaperAsset } from "../../lib/useMachineAppearance";
import { Button } from "../shared/Button";

const choices = [
  ["none", "None"], ["dots", "Dots"], ["domios", "Domios"], ["space", "Space"], ["botanical", "Botanical"],
] as const;

export function QuickWallpaper({ config, onOpenLab }: { config: AppConfig; onOpenLab(): void }) {
  const selected = useContext(MachineAppearanceContext);
  const theme = selected?.appearance.theme ?? config.theme;
  const wallpaper = (selected ? selected.appearance.wallpaper : config.chat_wallpaper) ?? undefined;
  const active = effectiveWallpaper(theme, wallpaper);
  const identity = selected?.identity?.color;
  const light = isLightColor(theme.background);
  const group = useId();
  const asset = useWallpaperAsset(selected?.peerId, wallpaper?.asset);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const ready = selected?.ready ?? true;
  const style = (paper?: ChatWallpaper, url?: string) => ({
    "--ct-bg": theme.background,
    "--ct-fg": theme.foreground,
    ...wallpaperRenderStyle(theme, paper, light, url, identity),
  }) as CSSProperties;
  const choose = async (pattern: typeof choices[number][0]) => {
    if (!ready || busy) return;
    setBusy(true);
    setError("");
    try {
      const next = { background: { pattern } };
      await saveWallpaper(selected?.peerId, next);
      selected?.projectWallpaper(next);
    } catch (failure) {
      setError(appearanceError(failure));
    } finally {
      setBusy(false);
    }
  };
  return <div className="ct-quick-wallpaper">
    <div className="ct-quick-wallpaper__row">
      <div className="ct-quick-wallpaper__current">
        <span className="ct-quick-wallpaper__thumb" aria-hidden="true"><span className="ct-chat-wallpaper" style={style(wallpaper, asset?.url)} /></span>
        <div><strong>Chat wallpaper</strong><small>{wallpaperSummary(theme, wallpaper)}<br />on this machine</small></div>
      </div>
      <div role="radiogroup" aria-label="Quick wallpaper" className="ct-quick-wallpaper__choices">
        {choices.map(([pattern, label]) => {
          const checked = active.background.pattern === pattern;
          return <label key={pattern}>
            <input type="radio" name={group} className="sr-only" aria-label={label} checked={checked} disabled={busy || !ready} onChange={() => void choose(pattern)} />
            <span className="ct-quick-wallpaper__thumb" aria-hidden="true">
              <span className="ct-chat-wallpaper" style={style(checked ? wallpaper : { background: { pattern, scale: pattern === "dots" ? 20 : 110 } }, checked ? asset?.url : undefined)} />
            </span>
            {label}
          </label>;
        })}
      </div>
    </div>
    {error && <p role="alert" className="ct-appearance-error">{error}</p>}
    <div className="ct-quick-wallpaper__footer">
      <small>More patterns, colours, your own file, shadows and roundness live in the Lab.</small>
      <Button variant="outline" onClick={onOpenLab}><FlaskConical size={15} aria-hidden="true" /> Open Appearance Lab</Button>
    </div>
  </div>;
}
