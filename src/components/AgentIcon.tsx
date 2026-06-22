import type { CSSProperties } from "react";
import { useAgentDescriptor } from "../lib/useAgentDescriptor";
import { ProviderGlyph } from "./ProviderGlyph";

interface Props {
  providerId: string | null | undefined;
  className?: string;
  style?: CSSProperties;
  title?: string;
  working?: boolean;
}

export function AgentIcon({ providerId, className, style, title, working }: Props) {
  const descriptor = useAgentDescriptor(providerId);
  return (
    <ProviderGlyph
      iconSvg={descriptor?.icon_svg}
      iconColor={descriptor?.icon_color}
      iconHint={descriptor?.icon_hint}
      className={className}
      style={style}
      title={title}
      working={working}
    />
  );
}
