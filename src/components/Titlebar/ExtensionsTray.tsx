import { useCallback, useEffect, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import { ExtensionsIcon } from "../Icons";
import { PluginIcon } from "../Extensions/PluginIcon";
import { GlancePopover, type GlanceAnchor } from "../Extensions/GlancePopover";
import { derivePinned, selectExtension, type TrayPluginData } from "./extensionsTrayLogic";

type TrayPlugin = TrayPluginData;
interface Snapshot {
  plugins?: TrayPlugin[];
}
interface ConfigShape {
  plugins?: Record<string, { pinned?: boolean } | undefined>;
}

interface OpenGlance {
  plugin: TrayPlugin;
  anchor: GlanceAnchor;
}

/**
 * Browser-style extensions tray: pinned plugins show as a row of their own icons
 * in a rounded capsule, with the puzzle button (open Extensions) at the end.
 * Clicking a pinned icon opens its glance popover (a quick peek); plugins without
 * a glance deep-link straight to their detail. The puzzle opens Extensions.
 */
export function ExtensionsTray({ onOpen }: { onOpen: () => void }) {
  const [pinned, setPinned] = useState<TrayPlugin[]>([]);
  const [glance, setGlance] = useState<OpenGlance | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [snap, cfg] = await Promise.all([
        invoke<Snapshot>("plugin_manager_snapshot"),
        invoke<ConfigShape>("get_config"),
      ]);
      setPinned(derivePinned(snap?.plugins ?? [], cfg?.plugins ?? {}));
    } catch {
      /* daemon not ready — leave tray empty */
    }
  }, []);

  useEffect(() => {
    void refresh();
    let unlisten: (() => void) | null = null;
    listen("config-changed", () => void refresh()).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, [refresh]);

  const open = useCallback((id: string | null) => selectExtension(id, onOpen), [onOpen]);

  // Pinned icon → glance popover (if the plugin has one), else open its detail.
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

  const iconBtn =
    "flex items-center justify-center w-7 h-7 rounded-md transition-colors hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]";

  const popover = glance && (
    <GlancePopover
      pluginId={glance.plugin.id}
      displayName={glance.plugin.display_name || glance.plugin.id}
      anchor={glance.anchor}
      onClose={() => setGlance(null)}
      onOpenSettings={(id) => open(id)}
    />
  );

  if (pinned.length === 0) {
    return (
      <button type="button" data-testid="titlebar-extensions" onClick={() => open(null)} title="Extensions" className={iconBtn}>
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
      <span className="w-px h-4 mx-0.5 bg-[var(--ct-border-subtle,rgba(255,255,255,0.1))]" />
      <button
        type="button"
        data-testid="titlebar-extensions"
        onClick={() => open(null)}
        title="Extensions"
        className="flex items-center justify-center w-6 h-6 rounded-full transition-colors hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
      >
        <ExtensionsIcon size={14} />
      </button>
    </div>
  );
}
