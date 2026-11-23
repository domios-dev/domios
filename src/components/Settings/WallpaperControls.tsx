import { useId, type CSSProperties } from "react";
import type { ChatWallpaper } from "../../../gen/ChatWallpaper";
import type { WallpaperFillMode } from "../../../gen/WallpaperFillMode";
import type { WallpaperFillPreset } from "../../../gen/WallpaperFillPreset";
import type { WallpaperPalette } from "../../../gen/WallpaperPalette";
import { FILLS, PALETTES, wallpaperDefaultOpacity, wallpaperDefaultScale, wallpaperFill, wallpaperPalette } from "../../lib/chatWallpaper";
import { UI_BUTTON } from "../shared/design";

interface Props {
  wallpaper: ChatWallpaper;
  light: boolean;
  disabled: boolean;
  onDraft: (wallpaper: ChatWallpaper) => void;
  onChange: (wallpaper: ChatWallpaper) => void;
  onReset: () => void;
}

export function WallpaperControls({ wallpaper, light, disabled, onDraft, onChange, onReset }: Props) {
  const id = useId();
  const { background, fill, palette } = wallpaper;
  const pattern = background.pattern;
  const photo = pattern === "image";
  const lines = pattern !== "dots" && pattern !== "none" && !photo;
  const intensity = Math.round(100 * Math.min(photo ? 0.3 : 0.24,
    background.opacity ?? wallpaperDefaultOpacity(pattern, light, palette !== undefined)));
  const fillMode = fill?.mode ?? "none";

  function slider(key: "opacity" | "scale", label: string, value: number, min: number, max: number, unit: string) {
    const next = (input: HTMLInputElement) => ({ ...wallpaper,
      background: { ...background, [key]: input.valueAsNumber / (key === "opacity" ? 100 : 1) } });
    const text = value + unit;
    return <label className="ct-wallpaper-property">
      <span>{label}</span>
      <input type="range" min={min} max={max} value={value} disabled={disabled}
        aria-valuetext={text} onChange={event => onDraft(next(event.currentTarget))}
        onPointerUp={event => onChange(next(event.currentTarget))}
        onKeyUp={event => {
          if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"].includes(event.key)) onChange(next(event.currentTarget));
        }} />
      <output aria-hidden="true">{text}</output>
    </label>;
  }

  function selectFill(mode: "none" | WallpaperFillMode, preset = fill?.preset ?? "midnight") {
    onChange({ ...wallpaper, fill: mode === "none" ? undefined : { mode, preset } });
  }
  const swatches = (Object.keys(PALETTES) as WallpaperPalette[]);
  const presets = (Object.keys(FILLS) as WallpaperFillPreset[]);
  return <fieldset className="ct-wallpaper-controls" disabled={disabled} aria-label="Wallpaper controls">
    {lines && <div className="ct-wallpaper-property ct-wallpaper-property--swatches">
      <span>Line colour</span>
      <div role="radiogroup" aria-label="Line colour" className="ct-wallpaper-swatches">
        {[undefined, ...swatches].map(key => <label key={key ?? "theme"} className="ct-wallpaper-swatch">
          <input type="radio" name={id + "-palette"} className="sr-only" checked={key === palette}
            onChange={() => onChange({ ...wallpaper, palette: key, background: { ...background, color: undefined, colors: undefined } })} />
          <span className="ct-wallpaper-swatch__mark" style={{ background: wallpaperPalette(key, light) }} />
          {key ? PALETTES[key].label : "Theme ink"}
        </label>)}
      </div>
    </div>}
    {pattern !== "none" && slider("opacity", "Intensity", intensity, 0, photo ? 30 : 24, "%")}
    {pattern === "dots" && slider("scale", "Scale", Math.min(64, Math.max(8, background.scale ?? 20)), 8, 64, " px")}
    {lines && slider("scale", "Scale", Math.min(480, Math.max(240, background.scale ?? wallpaperDefaultScale(pattern))), 240, 480, " px")}
    {!photo && pattern !== "dots" && <div className="ct-wallpaper-property ct-wallpaper-property--swatches">
      <span>Background</span>
      <div className="ct-wallpaper-fills">
        <div className="ct-wallpaper-segments" role="group" aria-label="Background fill mode">
          {(["none", "solid", "gradient"] as const).map(mode => <button key={mode} type="button"
            aria-pressed={mode === fillMode} onClick={() => selectFill(mode)}>{mode[0].toUpperCase() + mode.slice(1)}</button>)}
        </div>
        <div className="ct-wallpaper-swatches ct-wallpaper-swatches--fill" role="radiogroup" aria-label="Background preset">
          {[undefined, ...presets].map(preset => <label key={preset ?? "none"} className="ct-wallpaper-swatch">
            <input type="radio" className="sr-only" name={id + "-fill"} checked={preset === fill?.preset}
              onChange={() => selectFill(preset ? fill?.mode ?? "gradient" : "none", preset)} />
            <span className="ct-wallpaper-swatch__mark" data-empty={preset === undefined || undefined} style={{
              background: preset ? wallpaperFill({ preset, mode: fill?.mode ?? "gradient" }, light) : undefined,
            } as CSSProperties} />
            {preset ? FILLS[preset].label : "None"}
          </label>)}
        </div>
      </div>
    </div>}
    <button type="button" className={UI_BUTTON.ghost + " ct-wallpaper-reset"} onClick={onReset}>Reset</button>
  </fieldset>;
}
