import type { CSSProperties } from "react";
import { useMemo } from "react";

interface Props {
  iconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
  className?: string;
  style?: CSSProperties;
  title?: string;
  working?: boolean;
}

/** Presentational provider glyph: inline SVG (or hint glyph fallback) at 1em,
 *  with the optional working-pulse animation. Source-agnostic — feed it icon
 *  fields from a ProviderDescriptor (agent) or the plugin snapshot (shell). */
export function ProviderGlyph({ iconSvg, iconColor, iconHint, className, style, title, working }: Props) {
  const html = useMemo(() => (iconSvg ? forceInlineSize(iconSvg) : null), [iconSvg]);

  const cls = ["agent-icon", working ? "agent-icon-working" : "", className]
    .filter(Boolean)
    .join(" ");

  const computedStyle: CSSProperties = {
    color: iconColor ?? "currentColor",
    display: "inline-block",
    width: "1em",
    height: "1em",
    lineHeight: 0,
    ...style,
  };

  if (!html) {
    return (
      <span className={cls} style={computedStyle} role="img" aria-label={title} title={title}>
        {iconHint ?? ""}
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
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function forceInlineSize(svg: string): string {
  return svg.replace(/<svg\b([^>]*)>/i, (_match, attrs: string) => {
    const cleaned = attrs
      .replace(/\swidth\s*=\s*"[^"]*"/i, "")
      .replace(/\sheight\s*=\s*"[^"]*"/i, "");
    return `<svg${cleaned} width="1em" height="1em">`;
  });
}
