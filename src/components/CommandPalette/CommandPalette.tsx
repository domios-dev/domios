import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { createPortal } from "react-dom";
import { isMac } from "../../lib/keybindings";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface CommandItem {
  id: string;
  label: string;
  icon?: string; // SVG path data (16x16 viewBox)
  shortcut?: string; // Display string like "Cmd+T"
  section?: string;
  action: () => void;
}

interface CommandPaletteProps {
  commands: CommandItem[];
}

// ---------------------------------------------------------------------------
// Icons (16x16 viewBox SVG path data)
// ---------------------------------------------------------------------------

const ICONS = {
  tab: "M2 3.5A1.5 1.5 0 0 1 3.5 2h9A1.5 1.5 0 0 1 14 3.5v9a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 12.5v-9ZM3.5 3a.5.5 0 0 0-.5.5V5h10V3.5a.5.5 0 0 0-.5-.5h-9ZM13 6H3v6.5a.5.5 0 0 0 .5.5h9a.5.5 0 0 0 .5-.5V6Z",
  close: "M3.72 3.72a.75.75 0 0 1 1.06 0L8 6.94l3.22-3.22a.75.75 0 1 1 1.06 1.06L9.06 8l3.22 3.22a.75.75 0 1 1-1.06 1.06L8 9.06l-3.22 3.22a.75.75 0 0 1-1.06-1.06L6.94 8 3.72 4.78a.75.75 0 0 1 0-1.06Z",
  workspace: "M1.75 1h12.5c.966 0 1.75.784 1.75 1.75v10.5A1.75 1.75 0 0 1 14.25 15H1.75A1.75 1.75 0 0 1 0 13.25V2.75C0 1.784.784 1 1.75 1Zm12.5 1.5H1.75a.25.25 0 0 0-.25.25v10.5c0 .138.112.25.25.25h12.5a.25.25 0 0 0 .25-.25V2.75a.25.25 0 0 0-.25-.25Z",
  settings: "M8 0a1 1 0 0 1 1 1v.1a6.97 6.97 0 0 1 2.186.86l.071-.07a1 1 0 0 1 1.415 1.414l-.072.07A6.97 6.97 0 0 1 13.46 5.5h.04a1 1 0 1 1 0 2h-.1a6.97 6.97 0 0 1-.86 2.186l.07.071a1 1 0 0 1-1.414 1.415l-.07-.072A6.97 6.97 0 0 1 9 11.96V12a1 1 0 1 1-2 0v-.1a6.97 6.97 0 0 1-2.186-.86l-.071.07a1 1 0 0 1-1.415-1.414l.072-.07A6.97 6.97 0 0 1 2.54 7.5H2.5a1 1 0 0 1 0-2h.1a6.97 6.97 0 0 1 .86-2.186L3.39 3.24a1 1 0 0 1 1.414-1.415l.07.072A6.97 6.97 0 0 1 7 1.04V1a1 1 0 0 1 1-1Zm0 5.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Z",
  sidebar: "M1 3.25A2.25 2.25 0 0 1 3.25 1h9.5A2.25 2.25 0 0 1 15 3.25v9.5A2.25 2.25 0 0 1 12.75 15h-9.5A2.25 2.25 0 0 1 1 12.75ZM3.25 2.5a.75.75 0 0 0-.75.75v9.5c0 .414.336.75.75.75H5v-11ZM6.5 2.5v11h6.25a.75.75 0 0 0 .75-.75v-9.5a.75.75 0 0 0-.75-.75Z",
  theme: "M8 1a7 7 0 1 0 0 14A7 7 0 0 0 8 1ZM1.5 8a6.5 6.5 0 1 1 13 0 6.5 6.5 0 0 1-13 0ZM8 3a5 5 0 0 0 0 10V3Z",
  claude: "M5.75 7.5a.75.75 0 0 1 .75.75v1.5a.75.75 0 0 1-1.5 0v-1.5a.75.75 0 0 1 .75-.75Zm5.25.75a.75.75 0 0 0-1.5 0v1.5a.75.75 0 0 0 1.5 0ZM6 1h4a4 4 0 0 1 4 4v6a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V5a4 4 0 0 1 4-4Z",
  cron: "M8 0a8 8 0 1 1 0 16A8 8 0 0 1 8 0ZM1.5 8a6.5 6.5 0 1 0 13 0 6.5 6.5 0 0 0-13 0ZM8 3.5a.75.75 0 0 1 .75.75v3.19l2.03 2.03a.75.75 0 0 1-1.06 1.06l-2.25-2.25A.75.75 0 0 1 7.25 8V4.25A.75.75 0 0 1 8 3.5Z",
  hierarchy: "M8 2a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3ZM8 6.5a.75.75 0 0 1 .75.75V9h2.5a.75.75 0 0 1 .75.75v1.5a1.5 1.5 0 1 1-1.5 0v-.75h-1.75v.75a1.5 1.5 0 1 1-1.5 0v-.75H5.5v.75a1.5 1.5 0 1 1-1.5 0V9.75A.75.75 0 0 1 4.75 9h2.5V7.25A.75.75 0 0 1 8 6.5Z",
  bell: "M8 16a2 2 0 0 0 1.985-1.75c.017-.137-.097-.25-.235-.25h-3.5c-.138 0-.252.113-.235.25A2 2 0 0 0 8 16ZM3 5a5 5 0 0 1 10 0c0 1.862.818 2.726 1.49 3.434.233.246.46.485.653.752.19.262.357.571.357.974 0 .593-.477 1.09-1.107 1.168C13.477 11.44 12.025 11.5 8 11.5c-4.025 0-5.477-.06-6.393-.172C.977 11.24.5 10.743.5 10.16c0-.403.167-.712.356-.974a6.96 6.96 0 0 0 .653-.752C2.182 7.726 3 6.862 3 5Z",
  search: "M11.5 7a4.5 4.5 0 1 1-9 0 4.5 4.5 0 0 1 9 0Zm-.82 4.74a6 6 0 1 1 1.06-1.06l3.04 3.04a.75.75 0 1 1-1.06 1.06l-3.04-3.04Z",
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Format shortcut for display: replaces Ctrl with Cmd on macOS */
function formatShortcut(shortcut: string): string {
  if (isMac) {
    return shortcut
      .replace(/Ctrl\+/g, "\u2318")
      .replace(/Shift\+/g, "\u21E7")
      .replace(/Alt\+/g, "\u2325");
  }
  return shortcut;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function CommandPalette({ commands }: CommandPaletteProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [animState, setAnimState] = useState<"entering" | "visible" | "exiting" | "hidden">("hidden");
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const animTimeoutRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Filter commands by query
  const filtered = useMemo(() => {
    if (!query.trim()) return commands;
    const q = query.toLowerCase();
    return commands.filter(
      (cmd) =>
        cmd.label.toLowerCase().includes(q) ||
        (cmd.section && cmd.section.toLowerCase().includes(q))
    );
  }, [commands, query]);

  // Clamp selected index when filter changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  // Open/close with animation
  const doOpen = useCallback(() => {
    setQuery("");
    setSelectedIndex(0);
    setOpen(true);
    setAnimState("entering");
    clearTimeout(animTimeoutRef.current);
    animTimeoutRef.current = setTimeout(() => setAnimState("visible"), 320);
  }, []);

  const doClose = useCallback(() => {
    setAnimState("exiting");
    clearTimeout(animTimeoutRef.current);
    animTimeoutRef.current = setTimeout(() => {
      setAnimState("hidden");
      setOpen(false);
    }, 150);
  }, []);

  // Focus input when opened
  useEffect(() => {
    if (open && animState === "entering") {
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open, animState]);

  // Global Cmd+K / Ctrl+K listener
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const modKey = isMac ? e.metaKey : e.ctrlKey;
      if (modKey && !e.shiftKey && !e.altKey && e.code === "KeyK") {
        e.preventDefault();
        e.stopPropagation();
        if (open) {
          doClose();
        } else {
          doOpen();
        }
      }
    };
    // Use capture to intercept before other handlers
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [open, doOpen, doClose]);

  // Keyboard navigation inside palette
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        doClose();
        return;
      }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((i) => (i + 1) % Math.max(1, filtered.length));
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((i) => (i - 1 + filtered.length) % Math.max(1, filtered.length));
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        const cmd = filtered[selectedIndex];
        if (cmd) {
          doClose();
          // Execute after close animation starts
          setTimeout(() => cmd.action(), 50);
        }
        return;
      }
    },
    [doClose, filtered, selectedIndex]
  );

  // Scroll selected item into view
  useEffect(() => {
    if (!listRef.current) return;
    const items = listRef.current.children;
    const item = items[selectedIndex] as HTMLElement | undefined;
    if (item) {
      item.scrollIntoView({ block: "nearest" });
    }
  }, [selectedIndex]);

  // Cleanup timeout on unmount
  useEffect(() => {
    return () => clearTimeout(animTimeoutRef.current);
  }, []);

  if (!open) return null;

  const isEntering = animState === "entering";
  const isExiting = animState === "exiting";

  // Group commands by section
  const sections: { name: string; items: CommandItem[] }[] = [];
  const sectionMap = new Map<string, CommandItem[]>();
  for (const cmd of filtered) {
    const sec = cmd.section || "Commands";
    if (!sectionMap.has(sec)) {
      sectionMap.set(sec, []);
      sections.push({ name: sec, items: sectionMap.get(sec)! });
    }
    sectionMap.get(sec)!.push(cmd);
  }

  // Compute flat index for items across sections
  let flatIndex = 0;

  return createPortal(
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 9999,
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: "min(20vh, 140px)",
      }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) doClose();
      }}
    >
      {/* Backdrop */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          background: "rgba(0, 0, 0, 0.5)",
          backdropFilter: "blur(24px)",
          WebkitBackdropFilter: "blur(24px)",
          opacity: isExiting ? 0 : 1,
          transition: isEntering
            ? "opacity 200ms cubic-bezier(0.16, 1, 0.3, 1)"
            : "opacity 120ms cubic-bezier(0.16, 1, 0.3, 1)",
        }}
      />

      {/* Panel */}
      <div
        onKeyDown={handleKeyDown}
        style={{
          position: "relative",
          width: 640,
          maxWidth: "calc(100vw - 48px)",
          maxHeight: 480,
          display: "flex",
          flexDirection: "column",
          background: "var(--ct-surface, var(--ct-bg))",
          border: "1px solid rgba(255, 255, 255, 0.1)",
          borderRadius: 12,
          boxShadow:
            "0 8px 16px rgba(0, 0, 0, 0.35), 0 24px 64px rgba(0, 0, 0, 0.3), 0 0 0 1px rgba(255, 255, 255, 0.06)",
          overflow: "hidden",
          transform: isExiting
            ? "scale(0.98) translateY(-4px)"
            : isEntering
            ? "scale(0.96) translateY(-8px)"
            : "scale(1) translateY(0)",
          opacity: isExiting ? 0 : isEntering ? 0 : 1,
          transition: isEntering
            ? "transform 320ms cubic-bezier(0.175, 0.885, 0.32, 1.1), opacity 320ms cubic-bezier(0.175, 0.885, 0.32, 1.1)"
            : "transform 120ms cubic-bezier(0.16, 1, 0.3, 1), opacity 120ms cubic-bezier(0.16, 1, 0.3, 1)",
          // Force animation start
          animation: isEntering ? "cp-panel-in 320ms cubic-bezier(0.175, 0.885, 0.32, 1.1) forwards" : undefined,
        }}
      >
        {/* Input area */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            padding: "12px 16px",
            gap: 10,
            borderBottom: "1px solid rgba(255, 255, 255, 0.06)",
          }}
        >
          {/* Search icon */}
          <svg
            width={16}
            height={16}
            viewBox="0 0 16 16"
            fill="currentColor"
            style={{ flexShrink: 0, opacity: 0.4 }}
          >
            <path fillRule="evenodd" d={ICONS.search} />
          </svg>

          <input
            ref={inputRef}
            type="text"
            placeholder="Type a command..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            style={{
              flex: 1,
              background: "transparent",
              border: "none",
              outline: "none",
              color: "var(--ct-fg)",
              fontSize: 15,
              fontFamily: "inherit",
              caretColor: "var(--ct-accent)",
              lineHeight: "24px",
            }}
          />

          {/* Escape hint */}
          <kbd
            style={{
              fontSize: 11,
              fontFamily: "var(--ct-mono, monospace)",
              opacity: 0.35,
              padding: "2px 6px",
              borderRadius: 4,
              border: "1px solid rgba(255, 255, 255, 0.1)",
              color: "var(--ct-fg)",
              lineHeight: "16px",
            }}
          >
            ESC
          </kbd>
        </div>

        {/* Results */}
        <div
          ref={listRef}
          style={{
            overflowY: "auto",
            padding: "6px 8px",
            flex: 1,
          }}
        >
          {filtered.length === 0 ? (
            <div
              style={{
                padding: "24px 16px",
                textAlign: "center",
                fontSize: 13,
                opacity: 0.4,
                color: "var(--ct-fg)",
              }}
            >
              No commands found
            </div>
          ) : (
            sections.map((section) => (
              <div key={section.name}>
                {/* Section header (only if multiple sections) */}
                {sections.length > 1 && (
                  <div
                    style={{
                      fontSize: 11,
                      fontWeight: 600,
                      textTransform: "uppercase",
                      letterSpacing: "0.05em",
                      opacity: 0.35,
                      padding: "8px 8px 4px",
                      color: "var(--ct-fg)",
                    }}
                  >
                    {section.name}
                  </div>
                )}

                {section.items.map((cmd) => {
                  const itemIndex = flatIndex++;
                  const isSelected = itemIndex === selectedIndex;
                  // Staggered animation delay: 20ms per item, capped at 100ms
                  const staggerDelay = Math.min(itemIndex * 20, 100);

                  return (
                    <button
                      key={cmd.id}
                      onMouseEnter={() => setSelectedIndex(itemIndex)}
                      onClick={() => {
                        doClose();
                        setTimeout(() => cmd.action(), 50);
                      }}
                      style={{
                        width: "100%",
                        display: "flex",
                        alignItems: "center",
                        gap: 10,
                        padding: "8px 8px",
                        borderRadius: 8,
                        border: "none",
                        cursor: "pointer",
                        background: isSelected ? "rgba(255, 255, 255, 0.07)" : "transparent",
                        color: "var(--ct-fg)",
                        fontSize: 13,
                        fontFamily: "inherit",
                        textAlign: "left",
                        transition: "background 80ms ease",
                        // Staggered fade-in
                        opacity: isEntering ? 0 : 1,
                        transform: isEntering ? "translateY(4px)" : "translateY(0)",
                        animation: animState === "entering" || animState === "visible"
                          ? `cp-item-in 200ms ease-out ${staggerDelay}ms forwards`
                          : undefined,
                      }}
                    >
                      {/* Icon */}
                      {cmd.icon && (
                        <svg
                          width={16}
                          height={16}
                          viewBox="0 0 16 16"
                          fill="currentColor"
                          style={{ flexShrink: 0, opacity: 0.5 }}
                        >
                          <path fillRule="evenodd" d={cmd.icon} />
                        </svg>
                      )}

                      {/* Label */}
                      <span style={{ flex: 1 }}>{cmd.label}</span>

                      {/* Shortcut */}
                      {cmd.shortcut && (
                        <span
                          style={{
                            fontSize: 11,
                            fontFamily: "var(--ct-mono, monospace)",
                            opacity: 0.5,
                            flexShrink: 0,
                          }}
                        >
                          {formatShortcut(cmd.shortcut)}
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </div>
      </div>

      {/* Keyframe animations injected via style tag */}
      <style>{`
        @keyframes cp-panel-in {
          from {
            opacity: 0;
            transform: scale(0.96) translateY(-8px);
          }
          to {
            opacity: 1;
            transform: scale(1) translateY(0);
          }
        }
        @keyframes cp-item-in {
          from {
            opacity: 0;
            transform: translateY(4px);
          }
          to {
            opacity: 1;
            transform: translateY(0);
          }
        }
      `}</style>
    </div>,
    document.body
  );
}

// ---------------------------------------------------------------------------
// Default command definitions (factory)
// ---------------------------------------------------------------------------

interface CommandActions {
  newTab: () => void;
  closeTab: () => void;
  newWorkspace: () => void;
  openSettings: () => void;
  toggleSidebar: () => void;
  spawnClaude: () => void;
  openCronPanel?: () => void;
  openAgentHierarchy?: () => void;
  openNotifications?: () => void;
}

export function buildDefaultCommands(actions: CommandActions): CommandItem[] {
  return [
    {
      id: "new-tab",
      label: "New Tab",
      icon: ICONS.tab,
      shortcut: "Ctrl+T",
      section: "Tabs",
      action: actions.newTab,
    },
    {
      id: "close-tab",
      label: "Close Tab",
      icon: ICONS.close,
      shortcut: "Ctrl+W",
      section: "Tabs",
      action: actions.closeTab,
    },
    {
      id: "new-workspace",
      label: "New Workspace",
      icon: ICONS.workspace,
      shortcut: "Ctrl+Shift+N",
      section: "Workspace",
      action: actions.newWorkspace,
    },
    {
      id: "settings",
      label: "Settings",
      icon: ICONS.settings,
      shortcut: "Ctrl+,",
      section: "App",
      action: actions.openSettings,
    },
    {
      id: "toggle-sidebar",
      label: "Toggle Sidebar",
      icon: ICONS.sidebar,
      section: "App",
      action: actions.toggleSidebar,
    },
    {
      id: "theme-switcher",
      label: "Theme Switcher",
      icon: ICONS.theme,
      section: "App",
      action: actions.openSettings, // Opens settings on Appearance tab
    },
    {
      id: "spawn-claude",
      label: "Spawn Claude",
      icon: ICONS.claude,
      section: "Agents",
      action: actions.spawnClaude,
    },
    {
      id: "cron-jobs",
      label: "Periodic Tasks",
      icon: ICONS.cron,
      section: "Tools",
      action: actions.openCronPanel || (() => {}),
    },
    {
      id: "agent-hierarchy",
      label: "Agent Hierarchy",
      icon: ICONS.hierarchy,
      section: "Agents",
      action: actions.openAgentHierarchy || (() => {}),
    },
    {
      id: "notifications",
      label: "Notifications",
      icon: ICONS.bell,
      section: "App",
      action: actions.openNotifications || (() => {}),
    },
  ];
}
