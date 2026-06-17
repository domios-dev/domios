import { useEffect, useRef } from "react";

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
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // Capture phase so the listener fires before any inner stopPropagation (xterm's canvas, child components) can swallow the event. Left + right
    // mousedown both close — `mousedown` covers right-click outside too.
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };

    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };

    document.addEventListener("mousedown", handleClickOutside, true);
    document.addEventListener("keydown", handleEscape, true);

    return () => {
      document.removeEventListener("mousedown", handleClickOutside, true);
      document.removeEventListener("keydown", handleEscape, true);
    };
  }, [onClose]);

  // Adjust position to keep menu within viewport
  useEffect(() => {
    if (!menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    if (rect.right > vw) {
      menuRef.current.style.left = `${x - rect.width}px`;
    }
    if (rect.bottom > vh) {
      menuRef.current.style.top = `${y - rect.height}px`;
    }
  }, [x, y]);

  return (
    <div
      ref={menuRef}
      style={{
        position: "fixed",
        left: x,
        top: y,
        zIndex: 9999,
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
  );
}
