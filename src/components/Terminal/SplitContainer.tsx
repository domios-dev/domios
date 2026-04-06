import { useCallback, useEffect, useRef, useState } from "react";
import { TerminalPane } from "./TerminalPane";
import { BrowserPane } from "../Browser/BrowserPane";

import { Divider } from "./Divider";
import { getAgentProvider } from "../../lib/agentProviders";
import type { SplitNode, Pane, ThemeConfig, FontConfig, TerminalConfigSettings } from "../../lib/types";
import { CloseXIcon } from "../Icons";

interface SplitContainerProps {
  node: SplitNode;
  panes: Record<string, Pane>;
  focusedPaneId: string | null;
  onFocusPane: (paneId: string) => void;
  onClosePane: (paneId: string) => void;
  onPtySpawned: (paneId: string, ptyId: string) => void;
  /** Path from root to this node, used for ratio updates */
  path: number[];
  /** Callback to update ratio at a given path */
  onUpdateRatio: (path: number[], ratio: number) => void;
  /** Theme configuration for terminal coloring */
  theme?: ThemeConfig;
  /** Font configuration */
  fontConfig?: FontConfig;
  /** Terminal settings */
  terminalConfig?: TerminalConfigSettings;
}

const MIN_PANE_SIZE = 100; // px

/** Count total leaf panes in a split tree */
function countLeaves(node: SplitNode): number {
  if (node.type === "Leaf") return 1;
  return node.children.reduce((sum, child) => sum + countLeaves(child), 0);
}

export function SplitContainer({
  node,
  panes,
  focusedPaneId,
  onFocusPane,
  onClosePane,
  onPtySpawned,
  path,
  onUpdateRatio,
  theme,
  fontConfig,
  terminalConfig,
}: SplitContainerProps) {
  // Determine if we are the root node (path is empty) and count panes
  const isRoot = path.length === 0;
  const hasSplits = isRoot ? countLeaves(node) > 1 : true;

  if (node.type === "Leaf") {
    const pane = panes[node.pane_id];
    if (!pane) return null;

    const agentProvider = getAgentProvider(pane.agent_type);
    const hasAgent = !!agentProvider;

    const splitButtons = pane.protected ? null : (
      <PaneToolbar
        hasAgent={hasAgent}
        onClose={() => onClosePane(pane.id)}
      />
    );

    if (pane.pane_type === "Browser") {
      return (
        <PaneWrapper paneId={pane.id} isFocused={focusedPaneId === pane.id} hasSplits={hasSplits}>
          <BrowserPane
            key={pane.id}
            paneId={pane.id}
            url={pane.url || ""}
            isFocused={focusedPaneId === pane.id}
            onFocus={onFocusPane}
          />
          {splitButtons}
        </PaneWrapper>
      );
    }

    // Terminal always renders; ChatView is an overlay on top
    return (
      <PaneWrapper paneId={pane.id} isFocused={focusedPaneId === pane.id} hasSplits={hasSplits}>
        <div className="relative w-full h-full">
          <TerminalPane
          key={pane.id}
          paneId={pane.id}
          ptyId={pane.pty_id}
          isFocused={focusedPaneId === pane.id}
          hasSplits={hasSplits}
          onFocus={onFocusPane}
          onPtySpawned={onPtySpawned}
          theme={theme}
          fontConfig={fontConfig}
          terminalConfig={terminalConfig}
        />
        </div>
        {splitButtons}
      </PaneWrapper>
    );
  }

  // Split node
  const { orientation, ratio, children } = node;
  const isHorizontal = orientation === "Horizontal";

  return (
    <SplitView
      isHorizontal={isHorizontal}
      ratio={ratio}
      path={path}
      onUpdateRatio={onUpdateRatio}
    >
      {children.map((child, i) => (
        <SplitContainer
          key={i}
          node={child}
          panes={panes}
          focusedPaneId={focusedPaneId}
          onFocusPane={onFocusPane}
          onClosePane={onClosePane}
          onPtySpawned={onPtySpawned}
          path={[...path, i]}
          onUpdateRatio={onUpdateRatio}
          theme={theme}
          fontConfig={fontConfig}
          terminalConfig={terminalConfig}
        />
      ))}
    </SplitView>
  );
}

// ---------------------------------------------------------------------------
// PaneWrapper -- handles materialize animation, focus indicator, close collapse
// ---------------------------------------------------------------------------

interface PaneWrapperProps {
  paneId: string;
  isFocused: boolean;
  hasSplits: boolean;
  children: React.ReactNode;
}

function PaneWrapper({ paneId, isFocused, hasSplits, children }: PaneWrapperProps) {
  const [isMaterialized, setIsMaterialized] = useState(false);
  const [focusPulse, setFocusPulse] = useState(false);
  const prevFocused = useRef(isFocused);

  // Materialize animation on mount
  useEffect(() => {
    // Only animate if there are splits (new pane appearing)
    if (hasSplits) {
      setIsMaterialized(true);
    }
  }, []);

  // Focus pulse when focus switches to this pane
  useEffect(() => {
    if (isFocused && !prevFocused.current && hasSplits) {
      setFocusPulse(true);
      const t = setTimeout(() => setFocusPulse(false), 400);
      return () => clearTimeout(t);
    }
    prevFocused.current = isFocused;
  }, [isFocused, hasSplits]);

  return (
    <div
      data-pane-id={paneId}
      className={`w-full h-full relative group/pane ${isMaterialized ? "pane-materialize" : ""} ${focusPulse ? "pane-focus-pulse" : ""}`}
      style={{
        // Focused pane outline — only when splits exist
        outline: hasSplits && isFocused ? "1.5px solid rgba(99, 102, 241, 0.5)" : "none",
        outlineOffset: hasSplits && isFocused ? "-1.5px" : undefined,
        // Unfocused dimming — only when splits exist
        filter: hasSplits && !isFocused ? "brightness(0.92)" : "none",
        transition: "filter 320ms cubic-bezier(0.16, 1, 0.3, 1), outline-color 320ms cubic-bezier(0.16, 1, 0.3, 1)",
      }}
    >
      {children}
    </div>
  );
}

// ---------------------------------------------------------------------------
// SplitView -- renders exactly 2 children with a draggable divider
// ---------------------------------------------------------------------------

interface SplitViewProps {
  isHorizontal: boolean;
  ratio: number;
  path: number[];
  onUpdateRatio: (path: number[], ratio: number) => void;
  children: React.ReactNode[];
}

function ConfirmStopDialog({ message, confirmLabel, onConfirm, onCancel }: { message?: string; confirmLabel?: string; onConfirm: () => void; onCancel: () => void }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onCancel}>
      <div
        className="bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg shadow-xl px-6 py-5 max-w-sm text-center"
        onClick={(e) => e.stopPropagation()}
      >
        <p className="text-sm text-[var(--ct-fg)] mb-4">{message || "Are you sure you want to stop the process?"}</p>
        <div className="flex items-center justify-center gap-3">
          <button
            onClick={onCancel}
            className="px-4 py-1.5 text-xs rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            className="px-4 py-1.5 text-xs rounded bg-[var(--ct-red)] hover:opacity-80 text-white font-medium transition-colors"
          >
            {confirmLabel || "Stop"}
          </button>
        </div>
      </div>
    </div>
  );
}

function PaneToolbar({
  hasAgent,
  onClose,
}: {
  hasAgent: boolean;
  onClose: () => void;
}) {
  const [confirmClose, setConfirmClose] = useState(false);

  const btnBase = "w-6 h-6 flex items-center justify-center rounded-md transition-all duration-150";

  return (
    <>
      <div className="absolute top-1.5 right-1.5 flex items-center gap-1 opacity-0 group-hover/pane:opacity-100 transition-opacity duration-200 z-10">
        {/* Close */}
        <button
          onClick={(e) => {
            e.stopPropagation();
            if (hasAgent) setConfirmClose(true);
            else onClose();
          }}
          className={`${btnBase} bg-[var(--ct-bg)]/90 backdrop-blur-sm border border-[var(--ct-border)]/60 text-[var(--ct-muted)] hover:bg-[var(--ct-red)]/80 hover:border-[var(--ct-red)] hover:text-white transition-all duration-150`}
          title={hasAgent ? "Close (agent running)" : "Close pane (Ctrl+Shift+Q)"}
        >
          <CloseXIcon size={8} />
        </button>
      </div>
      {confirmClose && (
        <ConfirmStopDialog
          onConfirm={() => { onClose(); setConfirmClose(false); }}
          onCancel={() => setConfirmClose(false)}
        />
      )}
    </>
  );
}

function SplitView({
  isHorizontal,
  ratio,
  path,
  onUpdateRatio,
  children,
}: SplitViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [localRatio, setLocalRatio] = useState<number | null>(null);
  const localRatioRef = useRef<number | null>(null);
  const activeRatio = localRatio ?? ratio;

  const handleDragStart = useCallback(() => {
    setIsDragging(true);
    setLocalRatio(ratio);
    localRatioRef.current = ratio;
  }, [ratio]);

  const handleDragEnd = useCallback(() => {
    setIsDragging(false);
    const final = localRatioRef.current;
    if (final !== null) {
      onUpdateRatio(path, final);
    }
    setLocalRatio(null);
    localRatioRef.current = null;
    window.dispatchEvent(new CustomEvent("codeterm-pane-resize"));
  }, [path, onUpdateRatio]);

  const handleDrag = useCallback(
    (delta: number) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const totalSize = isHorizontal ? rect.width : rect.height;
      if (totalSize <= 0) return;

      const deltaRatio = delta / totalSize;
      setLocalRatio((prev) => {
        const current = prev ?? ratio;
        const minRatio = MIN_PANE_SIZE / totalSize;
        const maxRatio = 1 - minRatio;
        const next = Math.max(minRatio, Math.min(maxRatio, current + deltaRatio));
        localRatioRef.current = next;
        return next;
      });
    },
    [isHorizontal, ratio],
  );

  const handleDoubleClick = useCallback(() => {
    onUpdateRatio(path, 0.5);
    setLocalRatio(null);
    requestAnimationFrame(() => {
      window.dispatchEvent(new CustomEvent("codeterm-pane-resize"));
    });
  }, [path, onUpdateRatio]);

  const first = children[0];
  const second = children[1];

  // Divider takes 1px now (was 6px with old hit area approach)
  const firstPercent = `${(activeRatio * 100).toFixed(4)}%`;
  const secondPercent = `${((1 - activeRatio) * 100).toFixed(4)}%`;

  return (
    <div
      ref={containerRef}
      style={{
        display: "flex",
        flexDirection: isHorizontal ? "row" : "column",
        width: "100%",
        height: "100%",
        overflow: "hidden",
        position: "relative",
      }}
    >
      <div
        style={{
          [isHorizontal ? "width" : "height"]: `calc(${firstPercent} - 1px)`,
          [isHorizontal ? "height" : "width"]: "100%",
          overflow: "hidden",
          minWidth: isHorizontal ? `${MIN_PANE_SIZE}px` : undefined,
          minHeight: !isHorizontal ? `${MIN_PANE_SIZE}px` : undefined,
        }}
      >
        {first}
      </div>
      <Divider
        orientation={isHorizontal ? "Horizontal" : "Vertical"}
        onDrag={handleDrag}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onDoubleClick={handleDoubleClick}
      />
      <div
        style={{
          [isHorizontal ? "width" : "height"]: `calc(${secondPercent} - 1px)`,
          [isHorizontal ? "height" : "width"]: "100%",
          overflow: "hidden",
          minWidth: isHorizontal ? `${MIN_PANE_SIZE}px` : undefined,
          minHeight: !isHorizontal ? `${MIN_PANE_SIZE}px` : undefined,
        }}
      >
        {second}
      </div>
      {/* Overlay to capture mouse events during drag (prevents iframe from stealing events) */}
      {isDragging && (
        <div
          style={{
            position: "absolute",
            inset: 0,
            zIndex: 20,
            cursor: isHorizontal ? "col-resize" : "row-resize",
          }}
        />
      )}
    </div>
  );
}
