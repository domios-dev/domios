import assert from "node:assert/strict";
import type { ChatWallpaper } from "../../gen/ChatWallpaper";
import { effectiveWallpaper, WALLPAPER_PATTERNS, wallpaperRenderStyle } from "./chatWallpaper";
import { DEFAULT_THEME } from "./themeCss";

const theme = { ...DEFAULT_THEME, chat_background: { pattern: "grid" } } as const;
const setting: ChatWallpaper = {
  background: { pattern: "domios", scale: 360, opacity: 0.8 },
  palette: "aurora",
  fill: { mode: "gradient", preset: "midnight" },
};
assert.equal(effectiveWallpaper(theme).background.pattern, "grid");
assert.equal(effectiveWallpaper(DEFAULT_THEME).background.pattern, "none");
assert.equal(effectiveWallpaper(theme, setting).background.pattern, "domios");
assert.equal(WALLPAPER_PATTERNS[2].id, "domios");
const dark = wallpaperRenderStyle(theme, setting, false);
assert.equal(dark["--ct-wallpaper-ink"], "linear-gradient(180deg, #7ee8fa, #80ff9a, #b49bff)");
assert.equal(dark["--ct-wallpaper-opacity"], "0.24");
assert.match(dark["--ct-wallpaper-mask"], /domios-domios\.svg.*360px 360px repeat/);
assert.equal(dark["--ct-wallpaper-fill"], "linear-gradient(160deg, var(--ct-bg), #141b33)");
assert.equal(wallpaperRenderStyle(theme, setting, true)["--ct-wallpaper-ink"],
  "linear-gradient(180deg, #0a7ea4, #2b8a3e, #6d48c8)");
// An unset machine keeps the plain background regardless of identity colour.
const unset = wallpaperRenderStyle(DEFAULT_THEME, undefined, false, undefined, "blue");
assert.doesNotMatch(Object.values(unset).join(" "), /ct-machine-/);
assert.equal(unset["--ct-wallpaper-mask"], "none");
for (const [light, opacity] of [[true, "0.035"], [false, "0.028"]] as const) {
  const dots = wallpaperRenderStyle(DEFAULT_THEME, { background: { pattern: "dots" } }, light);
  assert.equal(dots["--ct-wallpaper-opacity"], opacity);
}
assert.equal(wallpaperRenderStyle(theme, { ...setting, fill_strength: 0.4 }, false)["--ct-wallpaper-fill"],
  "linear-gradient(160deg, color-mix(in srgb, var(--ct-bg) 40%, var(--ct-bg)), color-mix(in srgb, #141b33 40%, var(--ct-bg)))");

const uploaded: ChatWallpaper = { background: { pattern: "image" }, asset: "f".repeat(64) + ".png" };
assert.doesNotMatch(wallpaperRenderStyle(theme, uploaded, true, "file:///example.png")["--ct-wallpaper-fill"], /file:/);
assert.match(wallpaperRenderStyle(theme, uploaded, true, "blob:http://localhost/fixture")["--ct-wallpaper-fill"],
  /var\(--ct-bg\) 70%.*center \/ cover/);
console.log("chat wallpaper rendering and precedence passed");
