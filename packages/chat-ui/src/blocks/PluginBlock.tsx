import { Puzzle } from "lucide-react";
import { BlockNotice } from "./BlockNotice";
import { ChatBlockFrame, type ChatBlockAction, type ChatBlockStatus } from "./ChatBlockFrame";
import { useChatBlockHost, type PluginBlockHost } from "./ChatBlockHost";
import { useLiveBinding } from "./liveBinding";
import {
  PLUGIN_STATUS_LABEL,
  pluginLifecycleResult,
  pluginPrimaryAction,
  pluginStatus,
  type PluginBlockInfo,
  type PluginBlockState,
} from "./pluginBlockModel";

const LOADING: PluginBlockState = { kind: "loading" };

const STATUS_TONE = { installed: "ok", update: "warn", "not-installed": "muted" } as const;

/** `@ds-plugin <id>`: a live plugin card whose Install/Update runs only from the user's click. */
export function PluginBlock({ pluginId, source }: { pluginId: string; source: string }) {
  const host = useChatBlockHost().plugins;
  const state = useLiveBinding(host?.binding, pluginId, LOADING);
  if (!host) return <PluginNotice source={source} title={pluginId} text="Plugin cards are unavailable in this view." />;
  switch (state.kind) {
    case "loading":
      return <PluginNotice source={source} title={pluginId} text="Checking your plugins…" />;
    case "unavailable":
      return <PluginNotice source={source} title={pluginId} text={`Plugin list unavailable: ${state.message}`} />;
    case "missing":
      return <PluginNotice source={source} title={pluginId} text={`No plugin named “${pluginId}” in your plugin channels.`} missing />;
    case "ready":
      return <PluginCard plugin={state.plugin} host={host} source={source} />;
  }
}

function PluginCard({ plugin, host, source }: { plugin: PluginBlockInfo; host: PluginBlockHost; source: string }) {
  const status = pluginStatus(plugin);
  const primary = pluginPrimaryAction(plugin);
  const chip: ChatBlockStatus = { label: PLUGIN_STATUS_LABEL[status], tone: STATUS_TONE[status] };
  const actions: ChatBlockAction[] = [];
  if (primary.kind === "install" || primary.kind === "update") {
    const kind = primary.kind;
    actions.push({
      id: kind,
      label: primary.label,
      busyLabel: kind === "install" ? "Installing…" : "Updating…",
      primary: true,
      disabled: primary.disabled,
      reason: primary.reason,
      run: async () => {
        await host.run(plugin.id, kind);
        return pluginLifecycleResult(kind, host.binding.read(plugin.id));
      },
    });
  } else if (primary.label) {
    actions.push({ id: "open", label: primary.label, primary: true, disabled: primary.disabled, reason: primary.reason, run: () => host.openView(plugin.id) });
  }
  const versions = [plugin.version && `Installed ${plugin.version}`, plugin.updateAvailable && plugin.remoteVersion && `Available ${plugin.remoteVersion}`].filter(Boolean);
  return (
    <ChatBlockFrame
      name="plugin"
      source={source}
      title={plugin.name}
      icon={host.renderIcon?.(plugin.id, 16) ?? <Puzzle size={14} aria-hidden="true" />}
      accent={plugin.iconColor}
      status={chip}
      summary={plugin.pitch || undefined}
      details={(
        <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
          <dt>Plugin id</dt><dd className="m-0 font-mono">{plugin.id}</dd>
          {versions.length > 0 && <><dt>Version</dt><dd className="m-0">{versions.join(" · ")}</dd></>}
          {primary.reason && <><dt>Note</dt><dd className="m-0">{primary.reason}</dd></>}
        </dl>
      )}
      actions={actions}
      fullPage={{ label: "Open in Plugins", open: () => host.openInPlugins(plugin.id) }}
      askText={`About the ${plugin.name} plugin (\`${source}\`)`}
    />
  );
}

function PluginNotice({ source, title, text, missing = false }: { source: string; title: string; text: string; missing?: boolean }) {
  return <BlockNotice name="plugin" source={source} label={`Plugin ${title}`} text={text} icon={Puzzle} missing={missing} />;
}
