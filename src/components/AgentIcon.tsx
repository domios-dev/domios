import { useSyncExternalStore, type CSSProperties } from "react";
import { useAgentDescriptor } from "../lib/useAgentDescriptor";
import { useTabProvider } from "../lib/useTabProvider";
import type { Tab } from "../lib/types";
import { ProviderGlyph } from "./ProviderGlyph";
import { cachedProviderIcon, subscribeProviderIcons } from "../lib/providerIconCache";

interface Props {
  tab?: Tab | null;
  profileId?: string;
  providerId: string | null | undefined;
  className?: string;
  style?: CSSProperties;
  title?: string;
  ariaLabel?: string;
  working?: boolean;
}

export function AgentIcon({ providerId, tab, profileId, className, style, title, ariaLabel, working }: Props) {
  const cachedIcon = useSyncExternalStore(subscribeProviderIcons, () => cachedProviderIcon(providerId), () => undefined);
  const liveDescriptor = useAgentDescriptor(providerId);
  const descriptor = liveDescriptor ?? cachedIcon;
  const provider = useTabProvider(tab);
  const profile = liveDescriptor?.agent_runtime?.launchProfiles?.[profileId ?? ""];
  return (
    <ProviderGlyph
      iconSvg={provider?.iconSvg ?? profile?.iconSvg ?? descriptor?.icon_svg}
      fallbackIconSvg={descriptor?.icon_svg}
      iconColor={descriptor?.icon_color}
      iconHint={descriptor?.icon_hint}
      className={className}
      style={style}
      title={title ?? provider?.displayName}
      ariaLabel={ariaLabel}
      working={working}
    />
  );
}
