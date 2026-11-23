import { CHAT_DOODLES } from "./chatDoodles";
import { isLightColor } from "./themeCss";
import { effectiveWallpaper, PALETTES, wallpaperDefaultOpacity } from "./chatWallpaper";
import { blendColor } from "./colors";
import type { AppearanceLook } from "./appearance";

const SVG = "http://www.w3.org/2000/svg";
const TILE_VIEWBOX = 512;
const MOTIF_LIMIT = 120;
const MOTIF_RADIUS = 30;
const DECEL = "cubic-bezier(.2,.7,.2,1)";

export function drawAppearanceDoodles(chat: HTMLElement, look: AppearanceLook, origin: [number, number], width: number, height: number,
  onFinish: () => void): (() => void) | undefined {
  const { background: wallpaper, palette } = effectiveWallpaper(look.theme, look.wallpaper ?? undefined);
  if (wallpaper.pattern !== "doodles" || wallpaper.opacity === 0 || width <= 0 || height <= 0 || typeof Element.prototype.animate !== "function") return;
  const tile = wallpaper.scale ?? 360;
  const scale = tile / TILE_VIEWBOX;
  const angle = (wallpaper.rotation ?? 0) * Math.PI / 180;
  const sin = Math.sin(angle), cos = Math.cos(angle);
  const light = isLightColor(look.theme.background);
  const stops = palette ? PALETTES[palette][light ? "light" : "dark"] : wallpaper.colors ?? [wallpaper.color ?? look.theme.foreground];
  const opacity = Math.min(look.wallpaper ? .24 : .12, wallpaper.opacity ?? wallpaperDefaultOpacity(wallpaper.pattern, light, Boolean(palette || wallpaper.colors || wallpaper.color)));
  const source = new DOMParser().parseFromString(CHAT_DOODLES, "image/svg+xml").documentElement;
  const motifs = Array.from(source.children).flatMap(node => {
    const translate = /^translate\(([\d.]+)\s+([\d.]+)\)$/.exec(node.getAttribute("transform") ?? "");
    return translate ? [{ node, center: [Number(translate[1]), Number(translate[2])] }] : [];
  });
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  for (const attribute of ["fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin"]) {
    svg.setAttribute(attribute, source.getAttribute(attribute) ?? "none");
  }
  svg.style.color = wallpaper.color ?? look.theme.foreground;
  svg.style.opacity = String(opacity);
  svg.classList.add("ct-appearance-doodles");
  chat.append(svg);
  const corner = Math.hypot(Math.max(Math.abs(origin[0]), Math.abs(width - origin[0])),
    Math.max(Math.abs(origin[1]), Math.abs(height - origin[1])));
  const animations: Animation[] = [];
  for (let top = 0; top < height && animations.length < MOTIF_LIMIT; top += tile) {
    for (let left = 0; left < width && animations.length < MOTIF_LIMIT; left += tile) {
      const tileSvg = document.createElementNS(SVG, "svg");
      tileSvg.setAttribute("x", String(left));
      tileSvg.setAttribute("y", String(top));
      tileSvg.setAttribute("width", String(tile));
      tileSvg.setAttribute("height", String(tile));
      tileSvg.setAttribute("viewBox", `0 0 ${TILE_VIEWBOX} ${TILE_VIEWBOX}`);
      tileSvg.setAttribute("overflow", "hidden");
      const group = document.createElementNS(SVG, "g");
      group.setAttribute("transform", `rotate(${wallpaper.rotation ?? 0} 256 256)`);
      tileSvg.append(group);
      svg.append(tileSvg);
      for (const motif of motifs) {
        if (animations.length >= MOTIF_LIMIT) break;
        const dx = motif.center[0] - 256, dy = motif.center[1] - 256;
        const x = (256 + dx * cos - dy * sin) * scale;
        const y = (256 + dx * sin + dy * cos) * scale;
        const radius = MOTIF_RADIUS * scale;
        if (x + radius < 0 || y + radius < 0 || x - radius > Math.min(tile, width - left) || y - radius > Math.min(tile, height - top)) continue;
        const item = document.importNode(motif.node, true);
        const position = Math.max(0, Math.min(1, (top + y) / height)) * (stops.length - 1);
        const stop = Math.floor(position);
        item.setAttribute("color", blendColor(stops[stop], stops[Math.min(stop + 1, stops.length - 1)], position - stop));
        const paths = item.querySelectorAll("path, rect, line, polyline, polygon, ellipse");
        for (const path of paths) {
          path.setAttribute("pathLength", "1");
          path.setAttribute("stroke-dasharray", "1");
        }
        group.append(item);
        const distance = Math.hypot(left + x - origin[0], top + y - origin[1]);
        const delay = 300 * Math.min(1, distance / Math.max(corner, 1));
        animations.push(item.animate(
          paths.length ? [{ strokeDashoffset: 1 }, { strokeDashoffset: 0 }] : [{ opacity: 0 }, { opacity: 1 }],
          { duration: 400, delay, fill: "both", easing: DECEL },
        ));
      }
    }
  }
  void Promise.allSettled(animations.map(animation => animation.finished)).then(onFinish);
  return () => {
    for (const animation of animations) animation.cancel();
    svg.remove();
  };
}
