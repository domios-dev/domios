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
