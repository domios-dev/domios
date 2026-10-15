import { InlineErrorNotice } from "../shared/ErrorNotice";
import { PluginPermissions } from "./PluginPermissions";
import { PluginNetworkAccess } from "./PluginNetworkAccess";
import {
  capabilityKeys,
  catalogKindLabel,
  primaryAction,
  type CatalogEntry,
} from "./pluginCatalog";
import { CapabilityChip, capabilityLabel } from "./CapabilityChip";
import { SchemaRenderer } from "../Settings/SchemaRenderer";
import { PresetManager } from "./PresetManager";
import { ChatPreprocessorPresetManager } from "./ChatPreprocessorPresetManager";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { AgentProviderScreen } from "../Settings/AgentsTab";
import { SecretBackendDetail } from "./SecretBackendDetail";
import { PluginView } from "./PluginView";
import { MarkdownDescription } from "./MarkdownDescription";
import { GroupLabel, type AppConfigShape } from "./shared";
import { invoke } from "../../lib/bridge";
import type { AppConfig, PluginActivity, ProviderConfig } from "../../lib/types";
import type { PluginActivityDisplayMap } from "../../lib/usePluginActivity";
import { Button } from "../shared/Button";
import { PluginDetailHeader } from "./PluginDetailHeader";

export type ActionKind = "install" | "update" | "remove" | "settings";
export type ActionOptions = { removeData?: boolean; allowHosts?: string[]; disallowHosts?: string[] };

export function PluginDetailPanel({
  entry,
  config,
  busy,
  onAction,
  onHostGrant,
  onSave,
  onUpdateFullConfig,
  onGone,
  activity,
  activityDisplayMap,
}: {
  entry: CatalogEntry;
  config: AppConfigShape | null;
  busy: string | null;
  activity?: PluginActivity;
  activityDisplayMap: PluginActivityDisplayMap;
  onAction: (kind: ActionKind, entry: CatalogEntry, options?: ActionOptions) => void;
  onHostGrant: (pluginId: string, host: string, allow: boolean) => void;
  onSave: (pluginId: string, key: string, value: unknown) => Promise<void>;
  onUpdateFullConfig: (next: AppConfig) => void | Promise<void>;
  onGone: () => void;
}) {
  // Keys off the transport-tagged descriptor, never a provider id.
  const descriptor = useAgentDescriptor(entry.id);
  const supportsPresets = descriptor?.behavior_config?.model === "presets_and_system_prompt";
  const permissions = entry.installed ? <PluginPermissions pluginId={entry.id} permissions={entry.sensitivePermissions} /> : null;
  const networkConfig = config?.plugins?.[entry.id];
  const networkBaseUrl = networkConfig?.baseUrl ?? networkConfig?.serverUrl;
  const networkAccess = entry.installed && entry.networkRequested ? (
    <PluginNetworkAccess
      entry={entry}
      baseUrl={typeof networkBaseUrl === "string" ? networkBaseUrl : ""}
      busy={busy === `settings:${entry.id}`}
      onChange={(host, allow) => onHostGrant(entry.id, host, allow)}
    />
  ) : null;

  // Agent plugins render their real provider settings via the shared screen.
  if (entry.capabilities?.agent_provider && config) {
    return (
      <div>
        <AgentProviderScreen
          providerId={entry.id}
          fullConfig={config as unknown as AppConfig}
          onUpdateFullConfig={(next) => void onUpdateFullConfig(next)}
          onProviderGone={onGone}
        />
        {permissions}
        {networkAccess}
      </div>
    );
  }

  // installed; before install, fall through to the generic install view).
  if (entry.capabilities?.secret_backend && entry.installed && config) {
    return (
      <div>
        <SecretBackendDetail entry={entry} config={config} onAction={onAction} actionBusy={busy} activity={activity} activityDisplayMap={activityDisplayMap} />
        {permissions}
        {networkAccess}
      </div>
    );
  }

  const action = primaryAction(entry);

  const schema = entry.settingsSchema ?? [];
  const current = (config?.plugins?.[entry.id] ?? {}) as Record<string, unknown>;
  const perms = entry.grantedPermissions ?? [];
  const capKeys = capabilityKeys(entry.capabilities);

  const onPluginConfigChange = (id: string, patch: Partial<ProviderConfig>) => {
    for (const [k, v] of Object.entries(patch)) void onSave(id, k, v);
  };

  return (
    <div data-testid="extensions-plugin-detail" data-plugin-id={entry.id}>
      <PluginDetailHeader
        entry={entry}
        busy={busy}
        onAction={onAction}
        activity={activity}
        activityDisplayMap={activityDisplayMap}
      />

      {entry.pendingReload && (
        <div
          role="status"
          data-testid="extensions-reload-required"
          style={{
            fontSize: 12,
            color: "var(--ct-warn)",
            background: "color-mix(in srgb, var(--ct-yellow) 12%, transparent)",
            border: "1px solid var(--ct-warn)",
            borderRadius: 8,
            padding: "8px 12px",
            marginBottom: 14,
          }}
        >
          Reload Domios to activate this plugin.
        </div>
      )}

      {entry.loadError && <div data-testid="extensions-plugin-load-error"><InlineErrorNotice error={{ code: entry.loadError.keptPrevious ? "plugin_reload_failed" : "plugin_load_failed", raw: `${entry.loadError.message}\n${entry.loadError.path}` }} /></div>}

      {action.reason && action.disabled && (
        <div data-testid="extensions-action-disabled-reason"><InlineErrorNotice error={action.reason} operation="changing this plugin" /></div>
      )}

      {entry.capabilities?.chat_backend && entry.installed && !entry.pendingReload && (
        <div style={{ marginBottom: 16 }}>
          <Button
            variant="primary"
            size="sm"
            data-testid="extensions-open-chat"
            data-plugin-id={entry.id}
            onClick={() => {
              void invoke("open_chat_backend_tab", { pluginId: entry.id }).catch((e) =>
                console.error("open_chat_backend_tab failed:", e),
              );
            }}
            title={`Open a chat tab backed by ${entry.displayName}`}
          >
            Open chat
          </Button>
        </div>
      )}

      {entry.capabilities?.view && entry.installed && !entry.pendingReload && config && (
        <section aria-label={`${entry.displayName} setup`} style={{ marginBottom: 18 }} data-testid="extensions-plugin-view" data-plugin-id={entry.id}>
          <PluginView pluginId={entry.id} api={config.api} embedded />
        </section>
      )}

      {(capKeys.length > 0 || perms.length > 0) && (
        <div style={{ marginBottom: 18 }} data-testid="extensions-capabilities">
          <GroupLabel>Capabilities</GroupLabel>
          <div style={{ display: "flex", flexWrap: "wrap", gap: "4px 12px" }}>
            {capKeys.map((c) => (
              <CapabilityChip key={`cap-${c}`} variant="system" label={c.replace(/_/g, " ")} title={`Capability: ${c}`} />
            ))}
            {perms.map((c) => (
              <CapabilityChip key={`perm-${c}`} label={capabilityLabel(c)} title={c} />
            ))}
          </div>
        </div>
      )}

      {permissions}
      {networkAccess}

      {entry.installed && schema.length > 0 && config && (
        <div style={{ marginBottom: 20 }} data-testid="extensions-settings">
          <GroupLabel>Settings</GroupLabel>
          <SchemaRenderer
            schema={schema}
            providerId={entry.id}
            config={config as unknown as AppConfig}
            values={current}
            onChange={onPluginConfigChange}
            hidePathLinks
          />
        </div>
      )}

      {entry.installed && !entry.pendingReload && supportsPresets && (
        <PresetManager pluginId={entry.id} models={descriptor?.available_models ?? []} />
      )}

      {entry.installed && !entry.pendingReload && entry.capabilities?.chat_preprocessor && (
        <ChatPreprocessorPresetManager pluginId={entry.id} />
      )}

      <GroupLabel>About</GroupLabel>
      {entry.readmeMarkdown ? (
        <MarkdownDescription markdown={entry.readmeMarkdown} />
      ) : entry.description ? (
        <p style={{ fontSize: 13, lineHeight: 1.6, color: "var(--ct-fg)", margin: 0 }} data-testid="extensions-description">{entry.description}</p>
      ) : (
        <p style={{ fontSize: 12.5, color: "var(--ct-muted)", margin: 0 }}>
          No description provided for this {catalogKindLabel(entry)}.
        </p>
      )}
      {entry.homepage && (
        <a
          data-testid="extensions-homepage-link"
          href={entry.homepage}
          target="_blank"
          rel="noreferrer noopener"
          style={{ display: "inline-block", marginTop: 10, fontSize: 12, color: "var(--ct-accent, #818cf8)" }}
        >
          {entry.homepage}
        </a>
      )}
    </div>
  );
}
