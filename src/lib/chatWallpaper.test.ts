import assert from "node:assert/strict";
import type { ChatWallpaper } from "../../gen/ChatWallpaper";
import { effectiveWallpaper, WALLPAPER_PATTERNS, wallpaperDefaultOpacity, wallpaperRenderStyle } from "./chatWallpaper";
import { DEFAULT_THEME } from "./themeCss";

// Dots chosen in the picker use the same near-invisible intensity as theme dots.
assert.equal(wallpaperDefaultOpacity("dots", true, false), 0.035);
assert.equal(wallpaperDefaultOpacity("dots", false, false), 0.028);

const theme = { ...DEFAULT_THEME, chat_background: { pattern: "grid" } } as const;
const setting: ChatWallpaper = {
  background: { pattern: "domios", scale: 360, opacity: 0.8 },
  palette: "aurora",
  fill: { mode: "gradient", preset: "midnight" },
};
assert.equal(effectiveWallpaper(theme).background.pattern, "grid");
assert.equal(effectiveWallpaper(DEFAULT_THEME).background.pattern, "none");
assert.equal(effectiveWallpaper(theme, setting).background.pattern, "domios");
assert.deepEqual(WALLPAPER_PATTERNS.slice(0, 3).map(pattern => pattern.id), ["none", "dots", "domios"]);
const dark = wallpaperRenderStyle(theme, setting, false);
assert.equal(dark["--ct-wallpaper-ink"], "linear-gradient(180deg, #7ee8fa, #80ff9a, #b49bff)");
assert.equal(dark["--ct-wallpaper-opacity"], "0.24");
assert.match(dark["--ct-wallpaper-mask"], /domios-domios\.svg.*360px 360px repeat/);
assert.equal(dark["--ct-wallpaper-fill"], "linear-gradient(160deg, var(--ct-bg), #141b33)");
assert.equal(wallpaperRenderStyle(theme, setting, true)["--ct-wallpaper-ink"],
  "linear-gradient(180deg, #0a7ea4, #2b8a3e, #6d48c8)");
const unset = wallpaperRenderStyle(DEFAULT_THEME, undefined, false, undefined, "blue");
assert.equal(unset["--ct-wallpaper-fill"], "none");
assert.equal(unset["--ct-wallpaper-mask"], "none");
assert.doesNotMatch(Object.values(unset).join(" "), /ct-machine-/);

const uploaded: ChatWallpaper = { background: { pattern: "image" }, asset: "f".repeat(64) + ".png" };
assert.doesNotMatch(wallpaperRenderStyle(theme, uploaded, true, "file:///example.png")["--ct-wallpaper-fill"], /file:/);
assert.match(wallpaperRenderStyle(theme, uploaded, true, "blob:http://localhost/fixture")["--ct-wallpaper-fill"],
  /var\(--ct-bg\) 70%.*center \/ cover/);
console.log("chat wallpaper rendering and precedence passed");
