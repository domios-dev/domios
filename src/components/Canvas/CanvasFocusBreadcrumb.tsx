interface CanvasFocusBreadcrumbProps {
  title: string | null;
  onBack: () => void;
}

export function CanvasFocusBreadcrumb({ title, onBack }: CanvasFocusBreadcrumbProps) {
  return (
    <div
      data-testid="canvas-breadcrumb"
      className="absolute top-2 left-2 z-20 flex items-center gap-1.5 rounded px-2.5 py-1 text-xs"
      style={{ background: "color-mix(in srgb, var(--ct-titlebar-bg) 92%, transparent)", border: "1px solid var(--ct-border-default)", color: "var(--ct-fg)", backdropFilter: "blur(8px)" }}
    >
      <span style={{ opacity: "var(--ct-text-secondary-opacity)" }}>Workspace</span>
      <span style={{ opacity: 0.5 }}>▸</span>
      <span style={{ fontWeight: 600, maxWidth: 220, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{title ?? "node"}</span>
      <button type="button" data-testid="canvas-breadcrumb-back" title="Back (Esc)" onClick={onBack} style={{ marginLeft: 4, padding: "0 6px", borderRadius: 4, border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer" }}>
        Esc
      </button>
    </div>
  );
}
