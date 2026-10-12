import type { CSSProperties } from "react";
import { Bot } from "lucide-react";
import { useProviderLogo } from "./useProviderLogo";

interface Props {
  iconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
  className?: string;
  style?: CSSProperties;
  title?: string;
  ariaLabel?: string;
  working?: boolean;
}

export function ProviderGlyph(props: Props) {
  return <ProviderGlyphContent key={props.iconSvg ?? ""} {...props} />;
}

function ProviderGlyphContent({ iconSvg, iconColor, className, style, title, ariaLabel, working }: Props) {
  const { presentation, onLoad, onError } = useProviderLogo(iconSvg);
  const html = iconSvg ? forceInlineSize(iconSvg) : null;

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

  if (presentation === "logo" && html) {
    return (
      <span
        className={cls}
        style={computedStyle}
        role="img"
        aria-label={ariaLabel ?? title ?? "Agent logo"}
        title={title}
        onErrorCapture={onError}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    );
  }

  const label = title ? `${title} default agent icon` : "Default agent icon";
  return (
    <span className={cls} style={{ ...computedStyle, position: "relative" }} role="img" aria-label={ariaLabel ?? label} title={title}>
      <Bot size="1em" aria-hidden="true" />
      {html && (
        <span
          aria-hidden="true"
          style={{ position: "absolute", inset: 0, visibility: "hidden" }}
          onLoadCapture={onLoad}
          onErrorCapture={onError}
          dangerouslySetInnerHTML={{ __html: html }}
        />
      )}
    </span>
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
