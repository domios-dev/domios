/**
 * Pane status pills contributed by `statusBubble` plugins (e.g. Git), shown in
 * the top-right row next to the primary pane pill (usage or chat model, tracking
 * its width so they sit just left of it; alone at the edge when there isn't one).
 *
 * A pill with an `action` (e.g. Git → `openPanel:git`) opens that host view on
 * click; without one, it opens the plugin's glance popover.
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { GlancePopover, type GlanceAnchor } from "./GlancePopover";
import { SELECT_EXTENSION_EVENT } from "../Titlebar/extensionsTrayLogic";
import type { Tone } from "./ViewNodeRenderer";

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
const EDGE = 8;
const GAP = 6;
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
  const [plugins, setPlugins] = useState<BubblePlugin[]>([]);
  const [pills, setPills] = useState<Record<string, BubbleData | null>>({});
  const [open, setOpen] = useState<{ id: string; name: string; configHelp?: string; anchor: GlanceAnchor } | null>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const [rightOffset, setRightOffset] = useState(EDGE);

  // Discover statusBubble plugins once.
  useEffect(() => {
    let alive = true;
    invoke<{ plugins?: SnapPlugin[] }>("plugin_manager_snapshot")
      .then((s) => {
        if (!alive) return;
        setPlugins(
          (s?.plugins ?? [])
            .filter((p) => p.capabilities?.status_bubble)
            .map((p) => ({ id: p.id, display_name: p.display_name, config_help: p.config_help })),
        );
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  // Poll each plugin's pill for this pane's context.
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

  // Sit just left of the primary pane pill, tracking its real width.
  useLayoutEffect(() => {
    const pane = wrapRef.current?.closest("[data-pane-id]");
    if (!pane) return;
    let primary: HTMLElement | null = null;
    const findPrimary = () => pane.querySelector(".ct-usage-bubble, .ct-chat-model-bubble") as HTMLElement | null;
    const update = () => {
      const next = findPrimary();
      if (next !== primary) {
        resizeObserver.disconnect();
        primary = next;
        if (primary) resizeObserver.observe(primary);
      }
      setRightOffset(primary ? EDGE + primary.offsetWidth + GAP : EDGE);
    };
    const resizeObserver = new ResizeObserver(update);
    update();
    const mutationObserver = new MutationObserver(update);
    mutationObserver.observe(pane, { childList: true, subtree: true });
    return () => {
      mutationObserver.disconnect();
      resizeObserver.disconnect();
    };
  }, [visible.length]);

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
      // Pane pills live at the top of the pane → open the popover downward.
      anchor: { right: window.innerWidth - r.right, top: r.bottom + 6 },
    });
  }, [pills, cwd]);

  if (visible.length === 0 && !open) return null;

  return (
    <div ref={wrapRef} style={{ position: "absolute", inset: 0, pointerEvents: "none", zIndex: 5 }}>
      <div
        style={{
          position: "absolute",
          top: "calc(6px + var(--ct-safe-top, 0px))",
          right: rightOffset,
          display: "flex",
          gap: GAP,
          pointerEvents: "auto",
        }}
      >
        {visible.map((p) => {
          const b = pills[p.id] as BubbleData;
          const c = TONE[b.tone ?? "default"];
          return (
            <button
              key={p.id}
              onClick={(e) => onClick(p, e)}
              title={b.tooltip || p.display_name || p.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 5,
                height: 16,
                fontSize: 10.5,
                fontWeight: 560,
                padding: "0 8px",
                borderRadius: 999,
                color: c,
                background: "color-mix(in srgb, var(--ct-bg-elev, #1e1e2e) 80%, transparent)",
                border: `1px solid color-mix(in srgb, ${c} 32%, transparent)`,
                backdropFilter: "blur(10px) saturate(140%)",
                cursor: "pointer",
                whiteSpace: "nowrap",
              }}
            >
              {b.label}
            </button>
          );
        })}
      </div>
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
    </div>
  );
}
