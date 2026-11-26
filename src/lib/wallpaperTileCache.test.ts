import assert from "node:assert/strict";
import { test } from "node:test";
import { chatBackgroundToCSS, chatPatternTile } from "./chatBackgroundCss";
import { DEFAULT_THEME } from "./themeCss";
import { wallpaperRenderStyle } from "./chatWallpaper";
import { applyRasterizedWallpaper, parseSvgTileCss, planWallpaperRaster } from "./wallpaperTileCache";

test("chatBackgroundToCSS and chatPatternTile emit explicit SVG width and height", () => {
  const backdrop = chatBackgroundToCSS({ ...DEFAULT_THEME, chat_background: { pattern: "doodles", scale: 360 } }, false);
  const decodedBackdrop = decodeURIComponent(backdrop);
  assert.match(decodedBackdrop, /width="360" height="360"/);

  const tile = chatPatternTile({ pattern: "doodles" }, 320);
  assert.ok(tile);
  assert.match(decodeURIComponent(tile), /width="320" height="320"/);
});

test("parseSvgTileCss extracts SVG mask and data-URI fill tiles and ignores non-SVG patterns", () => {
  const mask = parseSvgTileCss('url("/assets/domios-agents.svg") 0 0 / 360px 360px repeat');
  assert.deepEqual(mask, { url: "/assets/domios-agents.svg", scale: 360, suffix: "" });

  const fillWithGradient = parseSvgTileCss(
    'url("data:image/svg+xml,%3Csvg%3E%3C/svg%3E") 0 0 / 360px 360px repeat, linear-gradient(135deg, #111, #222) no-repeat',
  );
  assert.deepEqual(fillWithGradient, {
    url: "data:image/svg+xml,%3Csvg%3E%3C/svg%3E",
    scale: 360,
    suffix: ", linear-gradient(135deg, #111, #222) no-repeat",
  });

  assert.equal(parseSvgTileCss("radial-gradient(circle, #fff 0.75px, transparent 1.1px) 10px 10px / 20px 20px repeat"), null);
  assert.equal(parseSvgTileCss("none"), null);
});

test("solid-ink SVG wallpapers promote to pre-tinted bitmap tiles without CSS mask compositing", () => {
  const style = {
    "--ct-fg": DEFAULT_THEME.foreground,
    ...wallpaperRenderStyle(DEFAULT_THEME, { background: { pattern: "doodles", scale: 360 } }, false),
  };
  const plan = planWallpaperRaster(style);
  assert.ok(plan);
  assert.equal(plan.slot, "mask");
  assert.equal(plan.tintColor, DEFAULT_THEME.foreground);

  const applied = applyRasterizedWallpaper(style, plan, "blob:tile-solid") as Record<string, string>;
  assert.equal(applied["--ct-wallpaper-mask"], "none");
  assert.equal(applied["--ct-wallpaper-ink"], 'url("blob:tile-solid") 0 0 / 360px 360px repeat');
});

test("gradient-ink SVG wallpapers keep gradient ink with a rasterized bitmap mask", () => {
  const style = {
    "--ct-fg": DEFAULT_THEME.foreground,
    ...wallpaperRenderStyle(
      DEFAULT_THEME,
      { background: { pattern: "domios", scale: 360 }, palette: "aurora" },
      false,
    ),
  };
  const plan = planWallpaperRaster(style);
  assert.ok(plan);
  assert.equal(plan.slot, "mask");
  assert.equal(plan.tintColor, undefined);

  const applied = applyRasterizedWallpaper(style, plan, "blob:tile-gradient") as Record<string, string>;
  assert.equal(applied["--ct-wallpaper-mask"], 'url("blob:tile-gradient") 0 0 / 360px 360px repeat');
  assert.match(applied["--ct-wallpaper-ink"], /^linear-gradient\(/);
});
