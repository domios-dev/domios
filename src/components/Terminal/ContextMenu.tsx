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
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };

    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };

    // Delay attaching to avoid the same right-click closing it immediately
    requestAnimationFrame(() => {
      document.addEventListener("mousedown", handleClickOutside);
      document.addEventListener("keydown", handleEscape);
    });

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleEscape);
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
        backgroundColor: "#1c2128",
        border: "1px solid #30363d",
        borderRadius: 6,
        padding: "4px 0",
        boxShadow: "0 8px 24px rgba(0,0,0,0.4)",
      }}
    >
      {actions.map((action, i) => (
        <div key={i}>
          {action.separator && i > 0 && (
            <div
              style={{
                height: 1,
                backgroundColor: "#30363d",
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
              color: "#c9d1d9",
              fontSize: 13,
              fontFamily: "inherit",
              cursor: "pointer",
              textAlign: "left",
            }}
            onMouseEnter={(e) => {
              (e.target as HTMLElement).style.backgroundColor = "#264f78";
            }}
            onMouseLeave={(e) => {
              (e.target as HTMLElement).style.backgroundColor = "transparent";
            }}
          >
            <span>{action.label}</span>
            {action.shortcut && (
              <span style={{ color: "#484f58", fontSize: 12, marginLeft: 16 }}>
                {action.shortcut}
              </span>
            )}
          </button>
        </div>
      ))}
    </div>
  );
}
