import { useCallback, useEffect, useRef } from "react";
import { X } from "lucide-react";
import { CanvasNodeCard } from "./CanvasNodeCard";
import type { LaidOutNode } from "./layout";
import type { SectionNavArgs } from "./nodeRegistry";

// UX-2 READ surface: a fullscreen, scrollable detail panel for one node (task / spec
// / plan). Opened by a card's ⤢ expand; it OVERLAYS the map (the camera never moves),
// so closing returns to the exact prior position for free. It reuses the existing W3
// card renderers (detail=true) — one source of truth for the full content, no
// duplicate — and supersedes the old in-card W3 read so the map stays schematic.

export interface CanvasDetailOverlayProps {
  nodeId: string | null;
  nodeById: Map<string, LaidOutNode>;
  resolveModel: (node: LaidOutNode) => unknown;
  onClose: () => void;
  onOpenPane?: (paneId: string) => void;
  onCopyLink?: (id: string) => void;
  onFocusStride?: (milestoneId: string) => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
}

function nodeTitle(node: LaidOutNode, model: unknown): string {
  const m = model as { task?: { name?: string }; title?: string } | undefined;
  if (node.kind === "task") return m?.task?.name ?? "Task";
  if (node.kind === "spec") return `${m?.title ?? "Spec"} · spec`;
  if (node.kind === "plan") return m?.title ?? "Plan";
  return m?.title ?? node.kind;
}

const FOCUSABLE = 'a[href],button:not([disabled]),input,textarea,select,[tabindex]:not([tabindex="-1"])';

export function CanvasDetailOverlay({
  nodeId,
  nodeById,
  resolveModel,
  onClose,
  onOpenPane,
  onCopyLink,
  onFocusStride,
  onSectionClick,
  onSectionFrame,
}: CanvasDetailOverlayProps) {
  const panelRef = useRef<HTMLDivElement | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);

  // Esc to close + a simple Tab focus-trap within the panel.
  const onKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key !== "Tab" || !panelRef.current) return;
      const items = Array.from(panelRef.current.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => el.offsetParent !== null);
      if (items.length === 0) return;
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    },
    [onClose],
  );

  useEffect(() => {
    if (!nodeId) return;
    returnFocusRef.current = (document.activeElement as HTMLElement | null) ?? null;
    const id = window.setTimeout(() => panelRef.current?.focus(), 0);
    return () => {
      window.clearTimeout(id);
      returnFocusRef.current?.focus?.();
    };
  }, [nodeId]);

  if (!nodeId) return null;
  const node = nodeById.get(nodeId);
  if (!node) return null;
  const model = resolveModel(node);
  if (model == null) return null;

  return (
    <div
      data-testid="canvas-detail-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={`${nodeTitle(node, model)} — detail`}
      onKeyDown={onKeyDown}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 40,
        display: "flex",
        alignItems: "stretch",
        justifyContent: "center",
        padding: "clamp(12px, 4vh, 56px) clamp(12px, 4vw, 64px)",
        background: "color-mix(in srgb, var(--ct-bg) 62%, transparent)",
        backdropFilter: "blur(3px)",
      }}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        data-testid="canvas-detail-panel"
        style={{
          position: "relative",
          width: "100%",
          maxWidth: node.kind === "task" ? 820 : 1040,
          maxHeight: "100%",
          display: "flex",
          flexDirection: "column",
          borderRadius: "var(--ct-radius-lg)",
          border: "1px solid var(--ct-border-default)",
          background: "var(--ct-bg)",
          boxShadow: "0 24px 80px -20px color-mix(in srgb, var(--ct-shadow, #000) 65%, transparent)",
          overflow: "hidden",
          outline: "none",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 12px 10px 16px", borderBottom: "1px solid var(--ct-border-subtle)", flexShrink: 0 }}>
          <span data-testid="canvas-detail-title" style={{ flex: 1, minWidth: 0, fontFamily: "var(--ct-font-ui)", fontWeight: 700, fontSize: "var(--ct-font-size-md, 15px)", color: "var(--ct-fg)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {nodeTitle(node, model)}
          </span>
          <span data-testid="canvas-detail-kind" style={{ fontSize: 11, fontWeight: 700, letterSpacing: "0.06em", textTransform: "uppercase", color: "color-mix(in srgb, var(--ct-fg) 55%, transparent)", flexShrink: 0 }}>
            {node.kind}
          </span>
          <button
            type="button"
            data-testid="canvas-detail-close"
            title="Close (Esc)"
            aria-label="Close detail"
            onClick={onClose}
            style={{ display: "flex", alignItems: "center", justifyContent: "center", width: 28, height: 28, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 }}
          >
            <X size={15} />
          </button>
        </div>
        <div data-testid="canvas-detail-body" style={{ position: "relative", flex: 1, minHeight: 0, overflow: "hidden" }}>
          <CanvasNodeCard
            node={node}
            model={model}
            lod="near"
            world="W3"
            crispScale={1}
            detail
            fillParent
            onOpenPane={onOpenPane}
            onCopyLink={onCopyLink}
            onFocusStride={onFocusStride}
            onSectionClick={onSectionClick}
            onSectionFrame={onSectionFrame}
          />
        </div>
      </div>
    </div>
  );
}
