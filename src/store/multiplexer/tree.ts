import { uid } from "../../lib/uid";
import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Pane,
  SplitNode,
  Orientation,
  Tab,
  Notification,
  PaneStatus,
} from "../../lib/types";

export interface ExtendedState extends MultiplexerState {
  notifications: Notification[];
  paneStatuses: Record<string, PaneStatus>;
  toasts: Notification[];
}

export function makeBrowserPane(url: string): Pane {
  return {
    id: uid(),
    pty_id: null,
    pane_type: "Browser",
    title: "Browser",
    cwd: null,
    url,
  };
}

export function makeTab(pane: Pane): Tab {
  return {
    id: uid(),
    title: "Terminal",
    split_tree: { type: "Leaf", pane_id: pane.id },
  };
}

/** Compare two TitleLevel values and return the higher-priority one. */
export function highestTitleLevel(a: TitleLevel, b: TitleLevel): TitleLevel {
  const levels = [TitleLevel.Default, TitleLevel.Auto, TitleLevel.Agent, TitleLevel.User];
  return levels.indexOf(a) >= levels.indexOf(b) ? a : b;
}

export function collectPaneIds(node: SplitNode): string[] {
  if (node.type === "Leaf") return [node.pane_id];
  return node.children.flatMap(collectPaneIds);
}

export function splitLeaf(
  node: SplitNode,
  targetPaneId: string,
  newPaneId: string,
  orientation: Orientation,
): SplitNode {
  if (node.type === "Leaf") {
    if (node.pane_id === targetPaneId) {
      return {
        type: "Split",
        orientation,
        ratio: 0.5,
        children: [
          { type: "Leaf", pane_id: targetPaneId },
          { type: "Leaf", pane_id: newPaneId },
        ],
      };
    }
    return node;
  }
  return {
    ...node,
    children: node.children.map((c) =>
      splitLeaf(c, targetPaneId, newPaneId, orientation),
    ),
  };
}

export function removeLeaf(node: SplitNode, targetPaneId: string): SplitNode | null {
  if (node.type === "Leaf") {
    return node.pane_id === targetPaneId ? null : node;
  }
  const filtered = node.children
    .map((c) => removeLeaf(c, targetPaneId))
    .filter((c): c is SplitNode => c !== null);
  if (filtered.length === 0) return null;
  if (filtered.length === 1) return filtered[0];
  return { ...node, children: filtered };
}

export function updateRatio(
  node: SplitNode,
  path: number[],
  newRatio: number,
): SplitNode {
  if (node.type === "Leaf") return node;
  if (path.length === 0) {
    return { ...node, ratio: newRatio };
  }
  const [head, ...tail] = path;
  return {
    ...node,
    children: node.children.map((c, i) =>
      i === head ? updateRatio(c, tail, newRatio) : c,
    ),
  };
}

/** Shorten terminal title for display — extract process name from full path */
export function shortenTitle(title: string): string {
  const pathMatch = title.match(/[/\\]([^/\\]+?)(?:\.exe)?$/i);
  if (pathMatch) {
    return pathMatch[1];
  }
  if (title.length > 50) {
    return title.slice(0, 47) + "...";
  }
  return title;
}
