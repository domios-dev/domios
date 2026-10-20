import { useEffect } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";

export interface ContextMenuAction {
  label: string;
  shortcut?: string;
  onClick: () => void;
  separator?: boolean;
}

interface ContextMenuProps {
  x: number;
  y: number;
  actions: ContextMenuAction[];
  onClose: () => void;
}

export function ContextMenu({ x, y, actions, onClose }: ContextMenuProps) {
  useEffect(() => {
    // Capture phase: the terminal swallows Escape before it would bubble to the popover.
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      onClose();
    };
    document.addEventListener("keydown", handleEscape, true);
    return () => document.removeEventListener("keydown", handleEscape, true);
  }, [onClose]);

  return (
    <AnchoredPopover open anchor={{ x, y }} gap={0} onClose={onClose}>
    <div
      data-testid="terminal-context-menu"
      style={{
        minWidth: 180,
        backgroundColor: "var(--ct-surface)",
        border: "1px solid var(--ct-border)",
        borderRadius: 6,
        padding: "4px 0",
        boxShadow: "var(--ct-shadow-lg)",
        backdropFilter: "blur(var(--ct-blur-overlay))",
        WebkitBackdropFilter: "blur(var(--ct-blur-overlay))",
      }}
    >
      {actions.map((action, i) => (
        <div key={i}>
          {action.separator && i > 0 && (
            <div
              style={{
                height: 1,
                backgroundColor: "var(--ct-border)",
                margin: "4px 0",
              }}
            />
          )}
          <button
            data-testid="terminal-context-menu-item"
            data-action-label={action.label}
            onClick={() => {
              action.onClick();
              onClose();
            }}
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              width: "100%",
              padding: "6px 12px",
              border: "none",
              background: "none",
              color: "var(--ct-fg)",
              fontSize: 13,
              fontFamily: "inherit",
              cursor: "pointer",
              textAlign: "left",
            }}
            onMouseEnter={(e) => {
              (e.target as HTMLElement).style.backgroundColor = "var(--ct-active)";
            }}
            onMouseLeave={(e) => {
              (e.target as HTMLElement).style.backgroundColor = "transparent";
            }}
          >
            <span>{action.label}</span>
            {action.shortcut && (
              <span style={{ color: "var(--ct-muted)", fontSize: 12, marginLeft: 16 }}>
                {action.shortcut}
              </span>
            )}
          </button>
        </div>
      ))}
    </div>
    </AnchoredPopover>
  );
}
