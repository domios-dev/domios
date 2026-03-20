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

    if (pane.pane_type === "Browser") {
      return (
        <div data-pane-id={pane.id} className="w-full h-full">
          <BrowserPane
            key={pane.id}
            paneId={pane.id}
            url={pane.url || ""}
            isFocused={focusedPaneId === pane.id}
            onFocus={onFocusPane}
          />
        </div>
      );
    }

    return (
      <div data-pane-id={pane.id} className="w-full h-full">
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

function SplitView({
  isHorizontal,
  ratio,
  path,
  onUpdateRatio,
  children,
}: SplitViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [isDragging, setIsDragging] = useState(false);

  const handleDragStart = useCallback(() => {
    setIsDragging(true);
  }, []);

  const handleDragEnd = useCallback(() => {
    setIsDragging(false);
    // Dispatch custom event to trigger terminal refit after drag ends
    window.dispatchEvent(new CustomEvent("codeterm-pane-resize"));
  }, []);

  const handleDrag = useCallback(
    (delta: number) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const totalSize = isHorizontal ? rect.width : rect.height;
      if (totalSize <= 0) return;

      const deltaRatio = delta / totalSize;
      let newRatio = ratio + deltaRatio;

      // Enforce minimum pane size
      const minRatio = MIN_PANE_SIZE / totalSize;
      const maxRatio = 1 - minRatio;
      newRatio = Math.max(minRatio, Math.min(maxRatio, newRatio));

      onUpdateRatio(path, newRatio);
    },
    [isHorizontal, ratio, path, onUpdateRatio],
  );

  const handleDoubleClick = useCallback(() => {
    onUpdateRatio(path, 0.5);
    // Trigger refit after reset to 50/50
    requestAnimationFrame(() => {
      window.dispatchEvent(new CustomEvent("codeterm-pane-resize"));
    });
  }, [path, onUpdateRatio]);

  const first = children[0];
  const second = children[1];

  // For 2 children: first gets `ratio`, second gets `1-ratio`
  const firstPercent = `${(ratio * 100).toFixed(4)}%`;
  const secondPercent = `${((1 - ratio) * 100).toFixed(4)}%`;

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
