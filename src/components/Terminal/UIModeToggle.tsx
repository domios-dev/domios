interface Props {
  uiMode: "terminal" | "chat";
  onToggle: () => void;
  agentType?: string | null;
}

// Providers known to expose a JSONL parser. Extend as new plugins
// implement sessionJsonlPath + parseSessionDelta.
const CHAT_SUPPORTED = new Set(["claude"]);

export function UIModeToggle({ uiMode, onToggle, agentType }: Props) {
  if (!agentType || !CHAT_SUPPORTED.has(agentType)) return null;
  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        onToggle();
      }}
      className="absolute top-1 right-1 z-10 text-xs px-2 py-0.5 rounded"
      style={{
        background:
          "color-mix(in srgb, var(--ct-bg, #0d1117) 85%, transparent)",
        border:
          "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 40%, transparent)",
        color: "var(--ct-muted-fg, #8b949e)",
        opacity: 0.75,
      }}
      title={uiMode === "terminal" ? "Show as chat" : "Show as terminal"}
    >
      {uiMode === "terminal" ? "💬" : "▶"}
    </button>
  );
}
