import type { CSSProperties } from "react";
import { useMemo } from "react";
import { useAgentDescriptor } from "../lib/agentDescriptorCache";

interface Props {
  providerId: string | null | undefined;
  className?: string;
  style?: CSSProperties;
  title?: string;
  working?: boolean;
}

/**
 * Generic provider brand-mark renderer.
 *
 * Reads the SVG markup and color from the provider's descriptor (populated
 * by `useAgentProviders` via the sync cache). New plugins get a rendered
 * icon for free — just return `icon_svg()` from the trait. The host has
 * zero knowledge of any specific provider here.
 *
 * - Width/height are forced to `1em` regardless of what the plugin's SVG
 *   declared, so the icon scales with the parent's font-size.
 * - The plugin's `icon_color` becomes the inline `color`, which the SVG
 *   inherits via `currentColor`. Caller-supplied `style` overrides both.
 * - When `working` is true, applies the universal working animation
 *   defined in globals.css under `.agent-icon-working`. Plugins that want
 *   custom animation can ship a `<style>` block inside their `icon_svg`.
 */
export function AgentIcon({ providerId, className, style, title, working }: Props) {
  const descriptor = useAgentDescriptor(providerId);

  const html = useMemo(() => {
    if (!descriptor?.icon_svg) return null;
    return forceInlineSize(descriptor.icon_svg);
  }, [descriptor?.icon_svg]);

  const cls = ["agent-icon", working ? "agent-icon-working" : "", className]
    .filter(Boolean)
    .join(" ");

  const computedStyle: CSSProperties = {
    color: descriptor?.icon_color ?? "currentColor",
    display: "inline-block",
    width: "1em",
    height: "1em",
    lineHeight: 0,
    ...style,
  };

  if (!html) {
    // Fall back to icon_hint character (or hidden if absent) until the
    // descriptor cache populates. Avoids layout jump.
    const hint = descriptor?.icon_hint ?? "";
    return (
      <span className={cls} style={computedStyle} role="img" aria-label={title} title={title}>
        {hint}
      </span>
    );
  }

  return (
    <span
      className={cls}
      style={computedStyle}
      role="img"
      aria-label={title}
      title={title}
      // Plugin-provided SVG. Plugins are local Rust code (not user data),
      // so dangerouslySetInnerHTML here carries no XSS exposure.
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

/**
 * Strip any explicit width/height attributes from the plugin's `<svg>`
 * tag and inject `width="1em" height="1em"`, so the host controls sizing.
 * Plugins are free to omit these (we'll add them) or specify any value
 * (we'll override). ViewBox is preserved.
 */
function forceInlineSize(svg: string): string {
  return svg.replace(/<svg\b([^>]*)>/i, (_match, attrs: string) => {
    const cleaned = attrs
      .replace(/\swidth\s*=\s*"[^"]*"/i, "")
      .replace(/\sheight\s*=\s*"[^"]*"/i, "");
    return `<svg${cleaned} width="1em" height="1em">`;
  });
}
