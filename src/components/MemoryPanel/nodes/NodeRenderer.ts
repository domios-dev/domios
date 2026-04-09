import type { MemNode } from "../types";
import type { Pulse } from "../useActivityPulses";

export interface NodeRendererProps {
  node: MemNode;
  selected: boolean;
  collapsed: boolean;
  scale: number;
  activePulse: Pulse | null;
  onSelect: (id: string) => void;
  onDragStart: (id: string, e: React.MouseEvent) => void;
  onToggleCollapse: (id: string) => void;
}
