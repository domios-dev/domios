/**
 * Pane status pills contributed by `statusBubble` plugins (e.g. Git), rendered
 * inline in the pane top bar. Each pill sizes to `--ct-pane-pill-h` so it shares
 * the bar's height rhythm with the mode switch and usage pill.
 *
 * A pill with an `action` (e.g. Git → `openPanel:git`) opens that host view on
 * click; without one, it opens the plugin's glance popover.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "../../lib/bridge";
import { GlancePopover, type GlanceAnchor } from "./GlancePopover";
import { SELECT_EXTENSION_EVENT } from "../Titlebar/extensionsTrayLogic";
import type { Tone } from "./ViewNodeRenderer";
import { ensurePluginSnapshot, usePluginSnapshotStore } from "../../lib/pluginSnapshotStore";

interface BubbleData {
  label: string;
  tone?: Tone;
  icon?: string | null;
  tooltip?: string | null;
  action?: string | null;
}
interface BubblePlugin {
  id: string;
  display_name?: string | null;
  config_help?: string | null;
}
interface SnapPlugin {
  id: string;
  display_name?: string | null;
  config_help?: string | null;
  capabilities?: { status_bubble?: boolean } | null;
}

const POLL_MS = 8000;
const TONE: Record<Tone, string> = {
  default: "var(--ct-fg)",
  muted: "var(--ct-muted)",
  ok: "var(--ct-ok, #4caf50)",
  warn: "var(--ct-warn, #e0a030)",
  danger: "var(--ct-err, #e57373)",
  accent: "var(--ct-accent, #5b8cff)",
};

function openPanel(action: string, cwd: string | null) {
  if (action.startsWith("openPanel:")) {
    window.dispatchEvent(
      new CustomEvent("codeterm:open-panel", { detail: { panel: action.slice("openPanel:".length), cwd } }),
    );
  } else if (action === "settings") {
    window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "extensions" } }));
  }
}

export function PluginBubbles({ cwd, paneId }: { cwd: string | null; paneId: string }) {
  const snapshotState = usePluginSnapshotStore();
  const [pills, setPills] = useState<Record<string, BubbleData | null>>({});
  const [open, setOpen] = useState<{ id: string; name: string; configHelp?: string; anchor: GlanceAnchor } | null>(null);
  const plugins = useMemo(
    () =>
      (snapshotState.snapshot.plugins as SnapPlugin[])
        .filter((p) => p.capabilities?.status_bubble)
        .map((p) => ({ id: p.id, display_name: p.display_name, config_help: p.config_help })),
    [snapshotState.snapshot.plugins],
  );

  useEffect(() => {
    void ensurePluginSnapshot({ background: snapshotState.lastSuccessfulSnapshot !== null }).catch(() => {});
  }, [snapshotState.lastSuccessfulSnapshot]);

  useEffect(() => {
    if (plugins.length === 0) return;
    let alive = true;
    const ctx = { cwd, paneId };
    const poll = () => {
      for (const p of plugins) {
        invoke<BubbleData | null>("plugin_bubble", { pluginId: p.id, ctx })
          .then((d) => alive && setPills((prev) => ({ ...prev, [p.id]: d })))
          .catch(() => {});
      }
    };
    poll();
    const t = setInterval(poll, POLL_MS);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [plugins, cwd, paneId]);

  const visible = plugins.filter((p) => pills[p.id]);

  const onClick = useCallback((p: BubblePlugin, e: React.MouseEvent) => {
    const pill = pills[p.id];
    if (pill?.action) {
      openPanel(pill.action, cwd);
      return;
    }
    const r = e.currentTarget.getBoundingClientRect();
    setOpen({
      id: p.id,
      name: p.display_name || p.id,
      configHelp: p.config_help ?? undefined,
      anchor: { right: window.innerWidth - r.right, top: r.bottom + 6 },
    });
  }, [pills, cwd]);

  if (visible.length === 0 && !open) return null;

  return (
    <>
      {visible.map((p) => {
        const b = pills[p.id] as BubbleData;
        const c = TONE[b.tone ?? "default"];
        return (
          <button
            key={p.id}
            data-testid={`plugin-bubble-${p.id}`}
            onClick={(e) => onClick(p, e)}
            onMouseDown={(e) => e.preventDefault()}
            title={b.tooltip || p.display_name || p.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 5,
              height: "var(--ct-pane-pill-h, 24px)",
              fontSize: 11,
              fontWeight: 560,
              padding: "0 10px",
              borderRadius: 999,
              color: c,
              background: "color-mix(in srgb, var(--ct-bg-elev, #1e1e2e) 80%, transparent)",
              border: `1px solid color-mix(in srgb, ${c} 32%, transparent)`,
              cursor: "pointer",
              whiteSpace: "nowrap",
            }}
          >
            {b.label}
          </button>
        );
      })}
      {open && (
        <GlancePopover
          pluginId={open.id}
          displayName={open.name}
          configHelp={open.configHelp}
          anchor={open.anchor}
          ctx={{ cwd, paneId }}
          onClose={() => setOpen(null)}
          onOpenSettings={(id) => {
            window.dispatchEvent(new CustomEvent(SELECT_EXTENSION_EVENT, { detail: { id } }));
            window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "extensions" } }));
          }}
        />
      )}
    </>
  );
}
