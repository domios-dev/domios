import { AgentIcon } from "../AgentIcon";
import { PluginIcon } from "../Extensions/PluginIcon";
import { getPluginProviderMeta } from "../../lib/pluginProviderMeta";

export function OriginProviderAvatar({ providerId }: { providerId: string | null | undefined }) {
  if (!providerId) return null;
  return (
    <span
      className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full border"
      style={{
        background: "color-mix(in srgb, var(--ct-surface, #161b22) 86%, var(--ct-accent, #58a6ff) 14%)",
        borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
        color: "var(--ct-fg)",
      }}
      aria-hidden
    >
      <AgentIcon providerId={providerId} className="text-[15px]" />
    </span>
  );
}

export function OriginPluginAvatar({ pluginId, iconSvg }: { pluginId: string; iconSvg?: string | null }) {
  const meta = getPluginProviderMeta(pluginId);
  const resolvedIcon = iconSvg ?? meta?.iconSvg;
  return (
    <span
      className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full border"
      style={{
        background: "color-mix(in srgb, var(--ct-surface, #161b22) 86%, var(--ct-accent, #58a6ff) 14%)",
        borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
        color: "var(--ct-fg)",
      }}
      aria-hidden="true"
    >
      <PluginIcon plugin={{ icon_svg: resolvedIcon, icon_color: meta?.iconColor }} size={14} />
    </span>
  );
}

