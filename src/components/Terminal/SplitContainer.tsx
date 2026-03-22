import { useCallback, useRef, useState } from "react";
import { TerminalPane } from "./TerminalPane";
import { BrowserPane } from "../Browser/BrowserPane";
import { Divider } from "./Divider";
import type { SplitNode, Pane, Orientation, ThemeConfig, FontConfig, TerminalConfigSettings } from "../../lib/types";

interface SplitContainerProps {
  node: SplitNode;
  panes: Record<string, Pane>;
  focusedPaneId: string | null;
  onFocusPane: (paneId: string) => void;
  onSplitPane: (paneId: string, orientation: Orientation) => void;
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

export function SplitContainer({
  node,
  panes,
  focusedPaneId,
  onFocusPane,
  onSplitPane,
  onClosePane,
  onPtySpawned,
  path,
  onUpdateRatio,
  theme,
  fontConfig,
  terminalConfig,
}: SplitContainerProps) {
  if (node.type === "Leaf") {
    const pane = panes[node.pane_id];
    if (!pane) return null;

    const splitButtons = (
      <PaneToolbar
        hasAgent={pane.agent_type === "claude"}
        onSplitH={() => onSplitPane(pane.id, "Horizontal")}
        onSplitV={() => onSplitPane(pane.id, "Vertical")}
        onClose={() => onClosePane(pane.id)}
      />
    );

    if (pane.pane_type === "Browser") {
      return (
        <div data-pane-id={pane.id} className="w-full h-full relative group/pane">
          <BrowserPane
            key={pane.id}
            paneId={pane.id}
            url={pane.url || ""}
            isFocused={focusedPaneId === pane.id}
            onFocus={onFocusPane}
          />
          {splitButtons}
        </div>
      );
    }

    return (
      <div data-pane-id={pane.id} className="w-full h-full relative group/pane">
        <TerminalPane
          key={pane.id}
          paneId={pane.id}
          ptyId={pane.pty_id}
          isFocused={focusedPaneId === pane.id}
          onFocus={onFocusPane}
          onPtySpawned={onPtySpawned}
          theme={theme}
          fontConfig={fontConfig}
          terminalConfig={terminalConfig}
        />
        {splitButtons}
      </div>
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
          onSplitPane={onSplitPane}
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
  onSplitH,
  onSplitV,
  onClose,
}: {
  hasAgent: boolean;
  onSplitH: () => void;
  onSplitV: () => void;
  onClose: () => void;
}) {
  const [confirmClose, setConfirmClose] = useState(false);

  const btnBase = "w-6 h-6 flex items-center justify-center rounded-md transition-all duration-150";
  const btnNormal = `${btnBase} bg-[var(--ct-bg)]/90 backdrop-blur-sm border border-[var(--ct-border)]/60 text-[var(--ct-muted)] hover:text-[var(--ct-fg)] hover:border-[var(--ct-muted)] hover:bg-[var(--ct-surface)]`;

  return (
    <>
      <div className="absolute top-1.5 right-1.5 flex items-center gap-1 opacity-0 group-hover/pane:opacity-100 transition-opacity duration-200 z-10">
        {/* Split right */}
        <button
          onClick={(e) => { e.stopPropagation(); onSplitH(); }}
          className={btnNormal}
          title="Split right (Ctrl+Shift+D)"
        >
          <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round">
            <rect x="1" y="1" width="10" height="10" rx="1.5" />
            <line x1="6" y1="1.5" x2="6" y2="10.5" />
          </svg>
        </button>
        {/* Split down */}
        <button
          onClick={(e) => { e.stopPropagation(); onSplitV(); }}
          className={btnNormal}
          title="Split down (Ctrl+Shift+E)"
        >
          <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round">
            <rect x="1" y="1" width="10" height="10" rx="1.5" />
            <line x1="1.5" y1="6" x2="10.5" y2="6" />
          </svg>
        </button>
        {/* Separator */}
        <div className="w-px h-3 bg-[var(--ct-border)]/50 mx-0.5" />

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
          <svg width="8" height="8" viewBox="0 0 10 10" fill="currentColor">
            <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
          </svg>
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
          [isHorizontal ? "width" : "height"]: `calc(${firstPercent} - 2px)`,
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
          [isHorizontal ? "width" : "height"]: `calc(${secondPercent} - 2px)`,
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
