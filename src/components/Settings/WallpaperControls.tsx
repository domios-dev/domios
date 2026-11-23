import { useId, type CSSProperties } from "react";
import type { ChatWallpaper } from "../../../gen/ChatWallpaper";
import type { ChatGradient } from "../../../gen/ChatGradient";
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
  const hasFill = Boolean(fill || background.gradient);
  const fillStrength = hasFill ? Math.round((wallpaper.fill_strength ?? 1) * 100) : 0;
  const withStrength = (input: HTMLInputElement) => ({ ...wallpaper, fill_strength: input.valueAsNumber / 100 });

  function slider(key: "opacity" | "scale" | "rotation", label: string, value: number, min: number, max: number, unit: string) {
    const next = (input: HTMLInputElement) => ({ ...wallpaper,
      background: { ...background, [key]: input.valueAsNumber / (key === "opacity" ? 100 : 1) } });
    const text = value + unit;
    return <label className="ct-wallpaper-property">
      <span>{label}</span>
      <input type="range" data-appearance-field={`chat_wallpaper.background.${key}`} min={min} max={max} value={value} disabled={disabled}
        aria-valuetext={text} onChange={event => onDraft(next(event.currentTarget))}
        onPointerUp={event => onChange(next(event.currentTarget))}
        onKeyUp={event => {
          if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"].includes(event.key)) onChange(next(event.currentTarget));
        }} />
      <output aria-hidden="true">{text}</output>
    </label>;
  }

  function selectFill(mode: "none" | WallpaperFillMode, preset = fill?.preset ?? "midnight") {
    onChange({ ...wallpaper, background: { ...background, gradient: undefined }, fill: mode === "none" ? undefined : { mode, preset } });
  }
  const lineStops = background.colors ?? [background.color ?? (light ? "#36404c" : "#b5c3d4")];
  const fillGradient: ChatGradient = background.gradient ?? { kind: "linear", stops: ["#34466d", "#7a95cb"], angle: 135 };
  const editStops = (stops: string[]) => onChange({ ...wallpaper, palette: undefined, background: { ...background, color: undefined, colors: stops } });
  const editGradient = (gradient: ChatGradient) => onChange({ ...wallpaper, fill: undefined, background: { ...background, gradient } });
  const swatches = (Object.keys(PALETTES) as WallpaperPalette[]);
  const presets = (Object.keys(FILLS) as WallpaperFillPreset[]);
  return <fieldset className="ct-wallpaper-controls" disabled={disabled} aria-label="Wallpaper controls">
    {lines && <div className="ct-wallpaper-property ct-wallpaper-property--swatches">
      <span>Line colour</span>
      <div role="radiogroup" aria-label="Line colour" className="ct-wallpaper-swatches">
        {[undefined, ...swatches].map(key => <label key={key ?? "theme"} className="ct-wallpaper-swatch">
          <input type="radio" data-appearance-field="chat_wallpaper.palette" name={id + "-palette"} className="sr-only" checked={key === palette}
            onChange={() => onChange({ ...wallpaper, palette: key, background: { ...background, color: undefined, colors: undefined } })} />
          <span className="ct-wallpaper-swatch__mark" style={{ background: wallpaperPalette(key, light) }} />
          {key ? PALETTES[key].label : "Theme ink"}
        </label>)}
      </div>
    </div>}
    {lines && <details>
      <summary>Custom line colours</summary>
      <div className="ct-wallpaper-property">
        <span>Gradient stops</span>
        <div className="ct-wallpaper-swatches">
          {lineStops.map((color, index) => <input key={index} type="color" data-appearance-field="chat_wallpaper.background.colors"
            aria-label={`Line colour ${index + 1}`} value={color} onChange={event => {
              const next = [...lineStops];
              next[index] = event.target.value;
              editStops(next);
            }} />)}
          <button type="button" disabled={lineStops.length >= 3} onClick={() => editStops([...lineStops, lineStops[lineStops.length - 1]])}>Add stop</button>
          <button type="button" disabled={lineStops.length < 2} onClick={() => editStops(lineStops.slice(0, -1))}>Remove stop</button>
        </div>
      </div>
    </details>}
    {pattern !== "none" && slider("opacity", "Intensity", intensity, 0, photo ? 30 : 24, "%")}
    {pattern === "dots" && slider("scale", "Scale", Math.min(64, Math.max(8, background.scale ?? 20)), 8, 64, " px")}
    {lines && slider("scale", "Scale", Math.min(480, Math.max(240, background.scale ?? wallpaperDefaultScale(pattern))), 240, 480, " px")}
    {lines && slider("rotation", "Rotation", background.rotation ?? 0, -180, 180, "°")}
    {!photo && pattern !== "dots" && <div className="ct-wallpaper-property ct-wallpaper-property--swatches">
      <span>Background</span>
      <div className="ct-wallpaper-fills">
        <div className="ct-wallpaper-segments" role="group" aria-label="Background fill mode">
          {(["none", "solid", "gradient"] as const).map(mode => <button key={mode} type="button" data-appearance-field="chat_wallpaper.fill.mode"
            aria-pressed={!background.gradient && mode === fillMode} onClick={() => selectFill(mode)}>{mode[0].toUpperCase() + mode.slice(1)}</button>)}
        </div>
        <div className="ct-wallpaper-swatches ct-wallpaper-swatches--fill" role="radiogroup" aria-label="Background preset">
          {[undefined, ...presets].map(preset => <label key={preset ?? "none"} className="ct-wallpaper-swatch">
            <input type="radio" data-appearance-field="chat_wallpaper.fill.preset" className="sr-only" name={id + "-fill"} checked={!background.gradient && preset === fill?.preset}
              onChange={() => selectFill(preset ? fill?.mode ?? "gradient" : "none", preset)} />
            <span className="ct-wallpaper-swatch__mark" data-empty={preset === undefined || undefined} style={{
              background: preset ? wallpaperFill({ preset, mode: fill?.mode ?? "gradient" }, light) : undefined,
            } as CSSProperties} />
            {preset ? FILLS[preset].label : "None"}
          </label>)}
        </div>
        <details>
          <summary>Custom background gradient</summary>
          <label>Shape <select data-appearance-field="chat_wallpaper.background.gradient.kind" value={fillGradient.kind} onChange={event =>
            editGradient({ ...fillGradient, kind: event.target.value === "radial" ? "radial" : "linear" })}>
            <option value="linear">Linear</option><option value="radial">Radial</option>
          </select></label>
          <label>Angle <input type="range" data-appearance-field="chat_wallpaper.background.gradient.angle" min={-180} max={180} value={fillGradient.angle ?? 135}
            onChange={event => editGradient({ ...fillGradient, angle: event.target.valueAsNumber })} /></label>
          <div className="ct-wallpaper-swatches">
            {fillGradient.stops.map((color, index) => <input key={index} type="color"
              data-appearance-field="chat_wallpaper.background.gradient.stops" aria-label={`Background colour ${index + 1}`} value={color} onChange={event => {
                const stops = [...fillGradient.stops];
                stops[index] = event.target.value;
                editGradient({ ...fillGradient, stops });
              }} />)}
            <button type="button" disabled={fillGradient.stops.length >= 4} onClick={() => editGradient({ ...fillGradient, stops: [...fillGradient.stops, fillGradient.stops[fillGradient.stops.length - 1]] })}>Add stop</button>
            <button type="button" disabled={fillGradient.stops.length <= 2} onClick={() => editGradient({ ...fillGradient, stops: fillGradient.stops.slice(0, -1) })}>Remove stop</button>
          </div>
        </details>
      </div>
    </div>}
    {!photo && pattern !== "dots" && <div className="ct-wallpaper-strength">
      <label className="ct-wallpaper-property">
        <span>Background strength</span>
        <input type="range" min={0} max={100} step={5} value={fillStrength} disabled={!hasFill || disabled}
          data-appearance-field="chat_wallpaper.fill_strength" aria-valuetext={`${fillStrength}%`} aria-describedby={id + "-fill-strength"}
          onChange={event => onDraft(withStrength(event.currentTarget))}
          onPointerUp={event => onChange(withStrength(event.currentTarget))}
          onKeyUp={event => {
            if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"].includes(event.key)) onChange(withStrength(event.currentTarget));
          }} />
        <output aria-hidden="true">{fillStrength}%</output>
      </label>
      <small id={id + "-fill-strength"}>{hasFill ? "0% is the plain theme background." : "Choose a solid or gradient background to use this."}</small>
    </div>}
    <button type="button" className={UI_BUTTON.ghost + " ct-wallpaper-reset"} onClick={onReset}>Reset</button>
  </fieldset>;
}
