import { useCallback, useEffect, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import { ExtensionsIcon } from "../Icons";
import { PluginIcon } from "../Extensions/PluginIcon";
import { GlancePopover, type GlanceAnchor } from "../Extensions/GlancePopover";
import { derivePinned, selectExtension, type TrayPluginData } from "./extensionsTrayLogic";
import { UI_TRAY_BTN, UI_TRAY_DIVIDER } from "../shared/design";
import { ensurePluginSnapshot, usePluginSnapshotStore } from "../../lib/pluginSnapshotStore";

type TrayPlugin = TrayPluginData;
interface ConfigShape {
  plugins?: Record<string, { pinned?: boolean } | undefined>;
}

interface OpenGlance {
  plugin: TrayPlugin;
  anchor: GlanceAnchor;
}

export function ExtensionsTray({ onOpen }: { onOpen: () => void }) {
  const snapshotState = usePluginSnapshotStore();
  const [pinned, setPinned] = useState<TrayPlugin[]>([]);
  const [glance, setGlance] = useState<OpenGlance | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [snap, cfg] = await Promise.all([
        ensurePluginSnapshot({ background: snapshotState.lastSuccessfulSnapshot !== null }),
        invoke<ConfigShape>("get_config"),
      ]);
      setPinned(derivePinned(snap?.plugins ?? [], cfg?.plugins ?? {}));
    } catch {
      /* daemon not ready — leave tray empty */
    }
  }, [snapshotState.lastSuccessfulSnapshot]);

  useEffect(() => {
    void refresh();
    let unlisten: (() => void) | null = null;
    listen("config-changed", () => void refresh()).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, [refresh]);

  useEffect(() => {
    const plugins = snapshotState.snapshot.plugins as TrayPlugin[];
    invoke<ConfigShape>("get_config")
      .then((cfg) => setPinned(derivePinned(plugins ?? [], cfg?.plugins ?? {})))
      .catch(() => {});
  }, [snapshotState.snapshot.plugins]);

  const open = useCallback((id: string | null) => selectExtension(id, onOpen), [onOpen]);

  const onPinnedClick = useCallback(
    (p: TrayPlugin, e: React.MouseEvent) => {
      if (!p.capabilities?.glance_view) {
        open(p.id);
        return;
      }
      const r = e.currentTarget.getBoundingClientRect();
      setGlance({ plugin: p, anchor: { top: r.bottom + 6, right: window.innerWidth - r.right } });
    },
    [open],
  );

  const iconBtn = UI_TRAY_BTN;

  const popover = glance && (
    <GlancePopover
      pluginId={glance.plugin.id}
      displayName={glance.plugin.display_name || glance.plugin.id}
      configHelp={glance.plugin.config_help ?? undefined}
      anchor={glance.anchor}
      onClose={() => setGlance(null)}
      onOpenSettings={(id) => open(id)}
    />
  );

  if (pinned.length === 0) {
    return (
      <button type="button" data-testid="titlebar-extensions" onClick={() => open(null)} title="Plugins" className={iconBtn}>
        <ExtensionsIcon size={14} />
      </button>
    );
  }

  return (
    <div className="flex items-center gap-0.5 h-7 px-1 rounded-full border border-[var(--ct-border-subtle,rgba(255,255,255,0.08))] bg-[var(--ct-bg-elev,rgba(255,255,255,0.03))]">
      {popover}
      {pinned.map((p) => (
        <button
          key={p.id}
          type="button"
          data-testid={`tray-plugin-${p.id}`}
          onClick={(e) => onPinnedClick(p, e)}
          title={p.display_name || p.id}
          className="flex items-center justify-center w-6 h-6 rounded-full transition-colors hover:bg-[var(--ct-hover)]"
        >
          <PluginIcon plugin={p} size={15} />
        </button>
      ))}
      <span className={UI_TRAY_DIVIDER} aria-hidden />
      <button
        type="button"
        data-testid="titlebar-extensions"
        onClick={() => open(null)}
        title="Plugins"
        className="flex items-center justify-center w-6 h-6 rounded-full transition-colors hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
      >
        <ExtensionsIcon size={14} />
      </button>
    </div>
  );
}
