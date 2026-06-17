/**
 * The pinned-plugin "glance" — a Chrome-extension-style popover that renders a
 * plugin's read-only `renderGlance()` ViewNode tree. Not settings: a quick peek
 * (Bitwarden account, transcriber status, …) with a gear to open full settings.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "../../lib/bridge";
import { GearIcon } from "../Icons";
import { GlanceBody, type GlanceView } from "./ViewNodeRenderer";

export interface GlanceAnchor {
  right: number;
  /** Anchor from the top (titlebar tray) or bottom (pane-footer bubble). */
  top?: number;
  bottom?: number;
}

export function GlancePopover({
  pluginId,
  displayName,
  anchor,
  ctx,
  onClose,
  onOpenSettings,
}: {
  pluginId: string;
  displayName: string;
  anchor: GlanceAnchor;
  ctx?: Record<string, unknown>;
  onClose: () => void;
  onOpenSettings: (pluginId: string) => void;
}) {
  const [view, setView] = useState<GlanceView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const load = useCallback(
    async (action?: string) => {
      try {
        const v = await invoke<GlanceView>("plugin_glance", { pluginId, action, ctx: ctx ?? {} });
        setView(v);
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [pluginId, ctx],
  );

  useEffect(() => {
    void load();
  }, [load]);

  // Dismiss on outside click + Escape.
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onDown, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  const onAction = useCallback(
    (action: string) => {
      if (action === "settings") {
        onOpenSettings(pluginId);
        onClose();
        return;
      }
      if (action.startsWith("openPanel:")) {
        const cwd = typeof ctx?.cwd === "string" ? ctx.cwd : null;
        window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: action.slice("openPanel:".length), cwd } }));
        onClose();
        return;
      }
      void load(action);
    },
    [pluginId, load, onOpenSettings, onClose, ctx],
  );

  return createPortal(
    <div
      ref={menuRef}
      className="fixed z-50"
      style={{
        top: anchor.top,
        bottom: anchor.bottom,
        right: anchor.right,
        width: 280,
        background: "color-mix(in srgb, var(--ct-bg-elev, #1e1e2e) 92%, transparent)",
        backdropFilter: "blur(24px) saturate(150%)",
        border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
        borderRadius: 12,
        boxShadow: "0 16px 48px rgba(0,0,0,0.45), inset 0 1px 0 rgba(255,255,255,0.06)",
        padding: 14,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <span style={{ fontSize: 13, fontWeight: 620 }}>{view?.title ?? displayName}</span>
        <button
          onClick={() => { onOpenSettings(pluginId); onClose(); }}
          title="Open settings"
          style={{ background: "transparent", border: "none", color: "var(--ct-muted)", cursor: "pointer", display: "flex", padding: 2 }}
        >
          <GearIcon size={14} />
        </button>
      </div>
      {error ? (
        <div style={{ fontSize: 12, color: "var(--ct-err, #e57373)" }}>{error}</div>
      ) : view ? (
        <GlanceBody view={view} onAction={onAction} />
      ) : (
        <div style={{ fontSize: 12, color: "var(--ct-muted)" }}>Loading…</div>
      )}
    </div>,
    document.body,
  );
}
