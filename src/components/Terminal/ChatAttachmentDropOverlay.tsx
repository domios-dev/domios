export function ChatAttachmentDropOverlay() {
  return (
    <div
      data-testid="web-drop-overlay"
      className="absolute inset-0 z-20 flex items-center justify-center border-2 border-dashed pointer-events-none"
      style={{
        borderColor: "var(--ct-accent, #58a6ff)",
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 70%, transparent)",
      }}
    >
      <span className="text-sm font-mono">Drop files to attach</span>
    </div>
  );
}
