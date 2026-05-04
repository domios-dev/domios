import { useCallback } from "react";
import type { AppConfig, ProviderConfig } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { SchemaRenderer } from "./SchemaRenderer";

/**
 * Render the Notifications section contributed by each active agent provider.
 *
 * Iterates `useAgentProviders()` and renders one `<SchemaRenderer />` per
 * provider whose `notifications_schema` is non-empty. Codex contributes
 * nothing today, so only Claude shows up. When a hypothetical third
 * provider plugin contributes a notification schema, it appears here
 * automatically — zero changes to this component.
 */
export function ProviderNotifications({
  fullConfig,
  onUpdateFullConfig,
}: {
  fullConfig: AppConfig;
  onUpdateFullConfig: (next: AppConfig) => void;
}) {
  const { providers } = useAgentProviders();

  const onProviderConfigChange = useCallback(
    (providerId: string, patch: Partial<ProviderConfig>) => {
      const prev = fullConfig.agents?.[providerId] ?? {};
      const nextProvider: ProviderConfig = { ...prev, ...patch };
      const nextAgents = { ...(fullConfig.agents ?? {}), [providerId]: nextProvider };
      onUpdateFullConfig({ ...fullConfig, agents: nextAgents });
    },
    [fullConfig, onUpdateFullConfig],
  );

  const contributors = providers.filter((p) => p.notifications_schema.length > 0);
  if (contributors.length === 0) return null;

  return (
    <>
      {contributors.map((p) => (
        <SchemaRenderer
          key={p.id}
          schema={p.notifications_schema}
          providerId={p.id}
          config={fullConfig}
          onChange={onProviderConfigChange}
        />
      ))}
    </>
  );
}
