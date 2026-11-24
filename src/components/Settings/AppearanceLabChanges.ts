import { DEFAULT_APPEARANCE, PALETTE_CONTROLS, TERMINAL_COLOURS, type AppearanceLook, type AppearanceTuning } from "../../lib/appearance";
import { effectiveWallpaper, FILLS, PALETTES, wallpaperSummary } from "../../lib/chatWallpaper";
import { presetThemeName } from "../../lib/machineIdentity";
import type { ThemeConfig } from "../../lib/types";

const colourControls = [...PALETTE_CONTROLS, ...TERMINAL_COLOURS.map(key => ({ key, label: key.replaceAll("_", " ") }))];
const shapeLabels: Record<string, string> = {
  roundness: "Roundness",
  shadow_strength: "Shadow",
  border_strength: "Border brightness",
  font_size: "Interface text",
};

export interface AppearanceLabChange {
  category: AppearanceLabCategory;
  label: string;
  before: string;
  after: string;
  revert: (look: AppearanceLook) => AppearanceLook;
}

export type AppearanceLabCategory = "presets" | "palette" | "wallpaper" | "shape" | "text";

function valueLabel(value: unknown): string {
  if (value == null) return "Default";
  if (typeof value === "number") return value.toFixed(2);
  return String(value);
}

function sameValue(before: unknown, after: unknown): boolean {
  const key = (value: unknown) => JSON.stringify(value, (_name, entry: unknown) => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) return entry;
    return Object.fromEntries(Object.entries(entry).sort(([left], [right]) => left.localeCompare(right)));
  });
  return before === after || key(before) === key(after);
}

export function appearanceLabChanges(current: AppearanceLook, draft: AppearanceLook, presets: ThemeConfig[]): AppearanceLabChange[] {
  const changes: AppearanceLabChange[] = [];
  const add = <Value>(category: AppearanceLabCategory, label: string, before: Value, after: Value, revert: AppearanceLabChange["revert"], display: (value: Value) => string = valueLabel) => {
    if (!sameValue(before, after)) changes.push({ category, label, before: display(before), after: display(after), revert });
  };

  const changedPreset = current.theme.name !== draft.theme.name;
  const paletteBaseline = changedPreset ? presets.find(preset => preset.name === draft.theme.name) : current.theme;
  if (changedPreset) {
    add("presets", "Preset", current.theme.name, draft.theme.name, look => {
      const theme = { ...current.theme, chat_background: look.theme.chat_background };
      if (paletteBaseline) {
        for (const { key } of colourControls) {
          if (look.theme[key] !== paletteBaseline[key]) theme[key] = look.theme[key];
        }
      }
      return { ...look, theme };
    }, presetThemeName);
  }
  if (paletteBaseline) {
    for (const { key, label } of colourControls) {
      add("palette", label, paletteBaseline[key], draft.theme[key], look => ({ ...look, theme: { ...look.theme, [key]: paletteBaseline[key] } }));
    }
  }
  add("wallpaper", "Theme wallpaper", current.theme.chat_background, draft.theme.chat_background,
    look => ({ ...look, theme: { ...look.theme, chat_background: current.theme.chat_background } }),
    value => value ? wallpaperSummary(value === current.theme.chat_background ? current.theme : draft.theme) : "None");

  const beforeTuning = current.tuning ?? DEFAULT_APPEARANCE;
  const afterTuning = draft.tuning ?? DEFAULT_APPEARANCE;
  for (const key of Object.keys(DEFAULT_APPEARANCE) as (keyof AppearanceTuning)[]) {
    add(key === "font_size" ? "text" : "shape", shapeLabels[key] ?? key.replaceAll("_", " "), beforeTuning[key], afterTuning[key], look => {
      const tuning = { ...look.tuning ?? DEFAULT_APPEARANCE, [key]: beforeTuning[key] };
      return { ...look, tuning: sameValue(tuning, beforeTuning) ? current.tuning : tuning };
    }, value => key === "font_size" ? `${13 + Number(value)} px` : valueLabel(value));
  }

  if (sameValue(current.wallpaper, draft.wallpaper)) return changes;

  const before = effectiveWallpaper(current.theme, current.wallpaper ?? undefined);
  const after = effectiveWallpaper(draft.theme, draft.wallpaper ?? undefined);
  const revertWallpaper = (look: AppearanceLook) => ({ ...look, wallpaper: current.wallpaper });
  const firstWallpaperChange = changes.length;
  if (draft.wallpaper && before.background.pattern === after.background.pattern && before.asset === after.asset
    && before.background.icon === after.background.icon && before.background.svg === after.background.svg) {
    const updateWallpaper = (look: AppearanceLook, patch: Partial<typeof before>) => {
      const wallpaper = { ...look.wallpaper ?? after, ...patch };
      return { ...look, wallpaper: sameValue(wallpaper, before) ? current.wallpaper : wallpaper };
    };
    const percent = (value: unknown) => value == null ? "Default" : `${Math.round(Number(value) * 100)}%`;
    for (const { key, label, unit } of [
      { key: "opacity", label: "Wallpaper intensity", unit: "%" },
      { key: "scale", label: "Wallpaper scale", unit: " px" },
      { key: "rotation", label: "Wallpaper rotation", unit: "°" },
    ] as const) {
      add("wallpaper", label, before.background[key], after.background[key], look => updateWallpaper(look, {
        background: { ...look.wallpaper?.background ?? after.background, [key]: before.background[key] },
      }), value => value == null || unit === "%" ? percent(value) : `${value}${unit}`);
    }
    const lineColour = (wallpaper: typeof before) => {
      const { background: { color, colors }, palette } = wallpaper;
      return { color, colors, palette };
    };
    add("wallpaper", "Wallpaper line colour", lineColour(before), lineColour(after), look => updateWallpaper(look, {
      palette: before.palette,
      background: { ...look.wallpaper?.background ?? after.background, color: before.background.color, colors: before.background.colors },
    }), value => {
      const { color, colors, palette } = value;
      return palette ? PALETTES[palette].label : colors?.join(", ") ?? color ?? "Theme ink";
    });
    const fill = (wallpaper: typeof before) => ({ fill: wallpaper.fill, gradient: wallpaper.background.gradient });
    add("wallpaper", "Wallpaper background", fill(before), fill(after), look => updateWallpaper(look, {
      fill: before.fill,
      background: { ...look.wallpaper?.background ?? after.background, gradient: before.background.gradient },
    }), value => {
      const { fill: preset, gradient } = value;
      if (preset) return `${FILLS[preset.preset].label} (${preset.mode})`;
      return gradient ? `${gradient.kind} · ${gradient.stops.join(", ")}${gradient.angle === undefined ? "" : ` · ${gradient.angle}°`}` : "None";
    });
    add("wallpaper", "Wallpaper background strength", before.fill_strength, after.fill_strength,
      look => updateWallpaper(look, { fill_strength: before.fill_strength }), percent);
  }
  if (changes.length === firstWallpaperChange) {
    changes.push({
      category: "wallpaper",
      label: "Wallpaper",
      before: current.wallpaper ? wallpaperSummary(current.theme, current.wallpaper) : "Theme wallpaper",
      after: draft.wallpaper ? wallpaperSummary(draft.theme, draft.wallpaper)
        : after.background.pattern === "none" && !after.background.gradient ? "None" : "Theme wallpaper",
      revert: revertWallpaper,
    });
  }
  return changes;
}
