import { useState, useEffect, useCallback, useRef, useMemo, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { FolderPlus, Network, Palette, PanelLeft, Search, Settings as SettingsIcon, SquarePlus, X } from "lucide-react";
import { isMac } from "../../lib/keybindings";
import { defaultSpawnProviderId, getAgentDescriptor } from "../../lib/agentDescriptorCache";
import { AgentIcon } from "../AgentIcon";
import { SECTION_ICONS } from "../Settings/sections";

const Bell = SECTION_ICONS.notifications;

// Types

export interface CommandItem {
  id: string;
  label: string;
  icon?: ReactNode;
  shortcut?: string; // Display string like "Cmd+T"
  section?: string;
  action: () => void;
}

interface CommandPaletteProps {
  commands: CommandItem[];
}

const ICON_SIZE = 16;
const ICONS = {
  tab: <SquarePlus size={ICON_SIZE} />,
  close: <X size={ICON_SIZE} />,
  workspace: <FolderPlus size={ICON_SIZE} />,
  settings: <SettingsIcon size={ICON_SIZE} />,
  sidebar: <PanelLeft size={ICON_SIZE} />,
  theme: <Palette size={ICON_SIZE} />,
  hierarchy: <Network size={ICON_SIZE} />,
  bell: <Bell size={ICON_SIZE} />,
};

// Helpers

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

// Component

export function CommandPalette({ commands }: CommandPaletteProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [animState, setAnimState] = useState<"entering" | "visible" | "exiting" | "hidden">("hidden");
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const animTimeoutRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const filtered = useMemo(() => {
    if (!query.trim()) return commands;
    const q = query.toLowerCase();
    return commands.filter(
      (cmd) =>
        cmd.label.toLowerCase().includes(q) ||
        (cmd.section && cmd.section.toLowerCase().includes(q))
    );
  }, [commands, query]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

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

  useEffect(() => {
    if (open && animState === "entering") {
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open, animState]);

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
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [open, doOpen, doClose]);

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
          setTimeout(() => cmd.action(), 50);
        }
        return;
      }
    },
    [doClose, filtered, selectedIndex]
  );

  useEffect(() => {
    if (!listRef.current) return;
    const items = listRef.current.children;
    const item = items[selectedIndex] as HTMLElement | undefined;
    if (item) {
      item.scrollIntoView({ block: "nearest" });
    }
  }, [selectedIndex]);

  useEffect(() => {
    return () => clearTimeout(animTimeoutRef.current);
  }, []);

  if (!open) return null;

  const isEntering = animState === "entering";
  const isExiting = animState === "exiting";

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
          <Search size={ICON_SIZE} aria-hidden="true" style={{ flexShrink: 0, opacity: 0.4 }} />

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
              fontFamily: "var(--ct-font-mono)",
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
                        opacity: isEntering ? 0 : 1,
                        transform: isEntering ? "translateY(4px)" : "translateY(0)",
                        animation: animState === "entering" || animState === "visible"
                          ? `cp-item-in 200ms ease-out ${staggerDelay}ms forwards`
                          : undefined,
                      }}
                    >
                      {/* Icon */}
                      {cmd.icon && (
                        <span aria-hidden="true" style={{ display: "flex", flexShrink: 0, opacity: 0.5, fontSize: ICON_SIZE }}>
                          {cmd.icon}
                        </span>
                      )}

                      {/* Label */}
                      <span style={{ flex: 1 }}>{cmd.label}</span>

                      {/* Shortcut */}
                      {cmd.shortcut && (
                        <span
                          style={{
                            fontSize: 11,
                            fontFamily: "var(--ct-font-mono)",
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

// Default command definitions (factory)

interface CommandActions {
  newTab: () => void;
  closeTab: () => void;
  newWorkspace: () => void;
  openSettings: () => void;
  toggleSidebar: () => void;
  spawnClaude: () => void;
  openAgentHierarchy?: () => void;
  openNotifications?: () => void;
}

export function buildDefaultCommands(actions: CommandActions): CommandItem[] {
  // Label reflects the capability-resolved default agent, not a hardcoded vendor.
  const defaultProvider = getAgentDescriptor(defaultSpawnProviderId());
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
      label: `Spawn ${defaultProvider?.display_name ?? "Agent"}`,
      icon: <AgentIcon providerId={defaultSpawnProviderId()} />,
      section: "Agents",
      action: actions.spawnClaude,
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
