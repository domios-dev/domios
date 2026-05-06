export interface ProjectMapNode {
  id: number;
  kind: string;
  display_name: string;
  serialized_name: string;
  path: string | null;
  start_line: number | null;
  start_col: number | null;
  end_line: number | null;
  end_col: number | null;
  signature?: string | null;
}

export interface ProjectMapEdge {
  source: number;
  target: number;
  kind: string;
  weight: number;
}

export interface ProjectMapStats {
  files: number;
  nodes: number;
  edges: number;
}

export interface ProjectMapPayload {
  nodes: ProjectMapNode[];
  edges: ProjectMapEdge[];
  stats: ProjectMapStats;
  truncated?: boolean;
}

export type FocusId = number | null;

export interface NodeFilter {
  kinds: Set<string>;
}
