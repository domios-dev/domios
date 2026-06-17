import { PuzzleIcon } from "../Icons";

export interface PluginIconData {
  icon_svg?: string | null;
  icon_color?: string | null;
  icon_hint?: string | null;
}

/** Force the inline SVG to a fixed pixel box (manifests omit width/height). */
export function sizeSvg(svg: string, size: number): string {
  let s = svg.replace(/<svg/, `<svg width="${size}" height="${size}"`);
  if (!/viewBox=/.test(s)) s = s.replace(/<svg/, '<svg viewBox="0 0 24 24"');
  return s;
}

/** Render a plugin's own icon: its manifest SVG (preferred), else a hint glyph,
 * else the generic puzzle SVG. No emoji — providers ship real SVGs. */
export function PluginIcon({ plugin, size = 18 }: { plugin: PluginIconData; size?: number }) {
  const color = plugin.icon_color || "var(--ct-fg)";
  if (plugin.icon_svg) {
    return (
      <span
        style={{ width: size, height: size, color, display: "inline-flex" }}
        dangerouslySetInnerHTML={{ __html: sizeSvg(plugin.icon_svg, size) }}
      />
    );
  }
  if (plugin.icon_hint && !/^[\u{1F000}-\u{1FAFF}\u{2600}-\u{27BF}]/u.test(plugin.icon_hint)) {
    return <span style={{ fontSize: size - 2, color }}>{plugin.icon_hint}</span>;
  }
  return <PuzzleIcon size={size} style={{ color }} />;
}
