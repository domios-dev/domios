/**
 * Tab status pills contributed by `statusBubble` plugins (e.g. Git), rendered
 * inline in the tab top bar. Each pill sizes to `--ct-tab-pill-h` so it shares
 * the bar's height rhythm with the mode switch and usage pill.
 *
 * A pill with an `action` (e.g. Git → `openPanel:git`) opens that host view on
 * click; without one, it opens the plugin's glance popover.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { GlancePopover, type GlanceAnchor } from "./GlancePopover";
import { SELECT_EXTENSION_EVENT } from "../Titlebar/extensionsTrayLogic";
import type { Tone } from "./ViewNodeRenderer";
import { ensurePluginSnapshot, usePluginSnapshotStore } from "../../lib/pluginSnapshotStore";
import { subscribePluginBubble, type BubbleData, type PluginBubbleSubscription } from "../../lib/pluginBubbleCache";
import { watchRepository, type RepoWatchState } from "../../lib/repoWatch";

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

const TONE: Record<Tone, string> = {
  default: "var(--ct-fg)",
  muted: "var(--ct-muted)",
  ok: "var(--ct-ok)",
  warn: "var(--ct-warn)",
  danger: "var(--ct-err)",
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

export function PluginBubbles({
  cwd,
  tabId,
  isActive,
}: {
  cwd: string | null;
  tabId: string;
  /** Only a visible tab computes its own bubbles; a hidden one rides along
   * on whatever its shared (plugin, repo) cache entry already has. */
  isActive: boolean;
}) {
  const snapshotState = usePluginSnapshotStore();
  const [pills, setPills] = useState<Record<string, BubbleData | null>>({});
  const [open, setOpen] = useState<{ id: string; name: string; configHelp?: string; anchor: GlanceAnchor } | null>(null);
  const [watch, setWatch] = useState<RepoWatchState | null>(null);
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

  const isActiveRef = useRef(isActive);
  isActiveRef.current = isActive;
  const subscriptionsRef = useRef<PluginBubbleSubscription[]>([]);

  // Subscribed once per (plugin, cwd, tab) identity — isActive changes go
  // through setActive below, never through re-subscribing.
  useEffect(() => {
    const subs = plugins.map((p) =>
      subscribePluginBubble(p.id, cwd, tabId, isActiveRef.current, (value) => {
        setPills((prev) => (prev[p.id] === value ? prev : { ...prev, [p.id]: value }));
      }),
    );
    subscriptionsRef.current = subs;
    return () => {
      for (const sub of subs) sub.unsubscribe();
      subscriptionsRef.current = [];
    };
  }, [plugins, cwd, tabId]);

  useEffect(() => {
    for (const sub of subscriptionsRef.current) sub.setActive(isActive);
  }, [isActive]);

  // Visible ⇒ exactly one watch lease; hidden or unmounted ⇒ exactly one
  // release. An unwatched repo is never rendered as live.
  useEffect(() => {
    if (!isActive || !cwd) {
      setWatch(null);
      return;
    }
    const lease = watchRepository(cwd, setWatch);
    return () => {
      lease.release();
      setWatch(null);
    };
  }, [isActive, cwd]);

  const visible = plugins.filter((p) => pills[p.id]);
  const watchReason = watch?.status === "unavailable" ? watch.reason : null;
  // Only a proven watch reads as live; pending and unwatched say so.
  const watchState = watchReason ? "unavailable" : (watch?.status ?? "idle");

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
        const base = b.tooltip || p.display_name || p.id;
        const c = watchReason ? TONE.muted : TONE[b.tone ?? "default"];
        const tooltip = watchReason
          ? `${base} — repository watch unavailable (${watchReason})`
          : watchState === "watching"
            ? base
            : `${base} — repository watch ${watchState}`;
        return (
          <button
            key={p.id}
            data-testid={`plugin-bubble-${p.id}`}
            data-watch-state={watchState === "watching" ? "live" : watchState}
            onClick={(e) => onClick(p, e)}
            onMouseDown={(e) => e.preventDefault()}
            title={tooltip}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 5,
              height: "var(--ct-tab-pill-h, 24px)",
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
          ctx={{ cwd, tabId }}
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
