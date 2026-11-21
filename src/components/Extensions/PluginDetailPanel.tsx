import { InlineErrorNotice } from "../shared/ErrorNotice";
import { PluginPermissions } from "./PluginPermissions";
import { PluginNetworkAccess } from "./PluginNetworkAccess";
import {
  capabilityKeys,
  catalogKindLabel,
  primaryAction,
  type CatalogEntry,
} from "./pluginCatalog";
import {
  CapabilityChip,
  capabilityLabel,
  permissionChipLabel,
  pluginCapabilityLabel,
} from "./CapabilityChip";
import { SchemaRenderer } from "../Settings/SchemaRenderer";
import { PresetManager } from "./PresetManager";
import { ChatPreprocessorPresetManager } from "./ChatPreprocessorPresetManager";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { AgentProviderScreen } from "../Settings/AgentsTab";
import { SecretBackendDetail } from "./SecretBackendDetail";
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
  settings = null,
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
  /** The plugin's own `config.yaml` settings, as `host.settingsJson()` returns them; null until loaded. */
  settings?: Record<string, unknown> | null;
  busy: string | null;
  activity?: PluginActivity;
  activityDisplayMap: PluginActivityDisplayMap;
  onAction: (kind: ActionKind, entry: CatalogEntry, options?: ActionOptions) => void;
  onHostGrant: (pluginId: string, host: string, allow: boolean) => void;
  onSave: (pluginId: string, values: Record<string, unknown>) => Promise<void>;
  onUpdateFullConfig: (next: AppConfig) => void | Promise<void>;
  onGone: () => void;
}) {
  // Keys off the transport-tagged descriptor, never a provider id.
  const descriptor = useAgentDescriptor(entry.id);
  const supportsPresets = descriptor?.behavior_config?.model === "presets_and_system_prompt";
  const permissions = entry.installed ? <PluginPermissions pluginId={entry.id} permissions={entry.sensitivePermissions} /> : null;
  // Schema settings live in config.yaml; the secret backend keeps its own config namespace.
  const networkConfig = settings ?? config?.plugins?.[entry.id];
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
  if (entry.installed && entry.capabilities?.agent_provider && config) {
    return (
      <div className="flex flex-col gap-4">
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
      <div className="flex flex-col gap-4">
        <SecretBackendDetail entry={entry} config={config} onAction={onAction} actionBusy={busy} activity={activity} activityDisplayMap={activityDisplayMap} />
        {permissions}
        {networkAccess}
      </div>
    );
  }

  const action = primaryAction(entry);

  const schema = entry.settingsSchema ?? [];
  const hasSectionWrapper = schema.some((field) => field.kind === "section");
  const current = settings ?? {};
  const perms = entry.grantedPermissions ?? [];
  const capKeys = capabilityKeys(entry.capabilities);

  const onPluginConfigChange = (id: string, patch: Partial<ProviderConfig>) => {
    void onSave(id, patch as Record<string, unknown>);
  };

  return (
    <div
      className="flex flex-col gap-4"
      data-testid="extensions-plugin-detail"
      data-plugin-id={entry.id}
    >
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
          className="rounded-[10px] border border-[color-mix(in_srgb,var(--ct-warn)_45%,transparent)] bg-[color-mix(in_srgb,var(--ct-yellow)_10%,var(--ct-bg))] px-3 py-2 text-xs text-[var(--ct-warn)]"
        >
          Reload Domios to activate this plugin.
        </div>
      )}

      {entry.loadError && !entry.loadError.keptPrevious && (
        <div data-testid="extensions-plugin-load-error">
          <InlineErrorNotice
            error={{ code: "plugin_load_failed", raw: `${entry.loadError.message}\n${entry.loadError.path}` }}
          />
        </div>
      )}

      {action.reason && action.disabled && (
        <p
          role="note"
          data-testid="extensions-action-disabled-reason"
          className="m-0 text-xs leading-relaxed text-[var(--ct-muted-fg)]"
        >
          {action.reason}
        </p>
      )}

      {((entry.capabilities?.chat_backend || entry.capabilities?.view) && entry.installed && !entry.pendingReload) && (
        <div className="flex flex-wrap items-center gap-2">
          {entry.capabilities?.chat_backend && (
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
          )}
          {entry.capabilities?.view && (
            <Button
              size="sm"
              onClick={() => window.dispatchEvent(new CustomEvent("codeterm:open-panel", {
                detail: { panel: `view:${entry.id}` },
              }))}
            >
              Open interface
            </Button>
          )}
        </div>
      )}

      {(capKeys.length > 0 || perms.length > 0) && (
        <section
          aria-label="Capabilities"
          className="flex flex-col gap-2"
          data-testid="extensions-capabilities"
        >
          <GroupLabel className="mb-0">Capabilities</GroupLabel>
          <div className="flex flex-wrap items-center gap-1.5">
            {capKeys.map((c) => (
              <CapabilityChip
                key={`cap-${c}`}
                variant="system"
                label={pluginCapabilityLabel(c)}
                title={`Capability: ${c}`}
              />
            ))}
            {perms.map((c) => (
              <CapabilityChip
                key={`perm-${c}`}
                label={permissionChipLabel(c)}
                title={capabilityLabel(c)}
              />
            ))}
          </div>
        </section>
      )}

      {permissions}
      {networkAccess}

      {entry.installed && schema.length > 0 && config && settings && (
        <section aria-label="Settings" data-testid="extensions-settings">
          <GroupLabel>Settings</GroupLabel>
          {hasSectionWrapper ? (
            <SchemaRenderer
              schema={schema}
              providerId={entry.id}
              config={config as unknown as AppConfig}
              values={current}
              onChange={onPluginConfigChange}
              hidePathLinks
            />
          ) : (
            <div className="ct-card ct-card--static p-4">
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
        </section>
      )}

      {entry.installed && !entry.pendingReload && supportsPresets && (
        <PresetManager pluginId={entry.id} models={descriptor?.available_models ?? []} />
      )}

      {entry.installed && !entry.pendingReload && entry.capabilities?.chat_preprocessor && (
        <ChatPreprocessorPresetManager pluginId={entry.id} />
      )}

      <section
        aria-label="About"
        className="ct-card ct-card--static p-4"
      >
        <GroupLabel>About</GroupLabel>
        {entry.readmeMarkdown ? (
          <MarkdownDescription markdown={entry.readmeMarkdown} />
        ) : entry.description ? (
          <p
            className="m-0 text-[13px] leading-relaxed text-[var(--ct-fg)]"
            data-testid="extensions-description"
          >
            {entry.description}
          </p>
        ) : (
          <p className="m-0 text-xs leading-relaxed text-[var(--ct-muted-fg)]">
            No description provided for this {catalogKindLabel(entry)}.
          </p>
        )}
        {entry.homepage && (
          <a
            data-testid="extensions-homepage-link"
            href={entry.homepage}
            target="_blank"
            rel="noreferrer noopener"
            className="mt-2.5 inline-block text-xs text-[var(--ct-accent)] hover:underline"
          >
            {entry.homepage}
          </a>
        )}
      </section>
    </div>
  );
}
