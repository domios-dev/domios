import { isChatUiSupported } from "../../lib/chatUiMode";

interface Props {
  uiMode: "terminal" | "chat";
  onToggle: () => void;
  agentType?: string | null;
}

export function UIModeToggle({ uiMode, onToggle, agentType }: Props) {
  if (!isChatUiSupported(agentType)) return null;
  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        onToggle();
      }}
      className="absolute top-1 right-1 z-20 text-xs px-2 py-0.5 rounded"
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
