export type MemKind =
  | "skill"
  | "fact"
  | "note"
  | "learning"
  | "rule"
  | "preference"
  | "conversation"
  | "script"
  | "peer"
  | "daily"
  | "summary"
  | string;

export interface MemNode {
  id: string;
  kind: MemKind;
  title: string;
  summary: string | null;
  area: string | null;
  importance: number | null;
  tags: string[] | null;
  has_vector: boolean;
  has_body: boolean;
  body?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
  access_count?: number;
  recurrence_count?: number;
}

export interface MemEdge {
  from: string;
  to: string;
  rel: string;
  confidence: number | null;
  valid_from: string | null;
  valid_to: string | null;
}

export interface GraphFullResponse {
  nodes: MemNode[];
  edges: MemEdge[];
  stats: { node_count: number; edge_count: number };
}

export interface MemItemFull {
  id: string;
  kind: string;
  scope: string;
  title: string;
  summary: string | null;
  area: string | null;
  tags: string[];
  body: string;
  source: string | null;
  importance: number;
  created_at: string;
  updated_at: string;
  accessed_at: string | null;
}

export type NodeKind = "basic" | "expanded" | "image";

export interface Viewport {
  panX: number;
  panY: number;
  scale: number;
}
