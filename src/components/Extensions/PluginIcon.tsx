import { Puzzle } from "lucide-react";

export interface PluginIconData {
  icon_svg?: string | null;
}

/** Force the inline SVG to a fixed pixel box (manifests omit width/height). */
export function sizeSvg(svg: string, size: number): string {
  let s = svg.replace(/<svg/, `<svg aria-hidden="true" width="${size}" height="${size}"`);
  if (!/viewBox=/.test(s)) s = s.replace(/<svg/, '<svg viewBox="0 0 24 24"');
  return s;
}

/** Render a host-sanitized plugin icon, or the generic puzzle fallback. */
export function PluginIcon({ plugin, size = 18 }: { plugin: PluginIconData; size?: number }) {
  if (plugin.icon_svg) {
    return (
      <span
        aria-hidden="true"
        style={{ width: size, height: size, color: "currentColor", display: "inline-flex" }}
        dangerouslySetInnerHTML={{ __html: sizeSvg(plugin.icon_svg, size) }}
      />
    );
  }
  return <Puzzle size={size} aria-hidden="true" style={{ color: "currentColor" }} />;
}
