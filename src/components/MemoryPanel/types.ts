export type MemAuthor = import("../../../gen/MemAuthor").MemAuthor;

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

type MemNodeWire = import("../../../gen/MemNode").MemNode;
export type MemNode = Omit<MemNodeWire, "author" | "importance" | "tags"> & Partial<Pick<MemNodeWire, "author">> & { [Field in "importance" | "tags"]: MemNodeWire[Field] | null } & {
  pinned?: boolean;
  body?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
  access_count?: number;
  recurrence_count?: number;
};

type MemEdgeWire = import("../../../gen/GraphEdge").GraphEdge;
export type MemEdge = Omit<MemEdgeWire, "valid_from"> & { valid_from: MemEdgeWire["valid_from"] | null };
export type GraphFullResponse = import("../../../gen/GraphFullResponse").GraphFullResponse;

export type MemItemFull = Pick<import("../../../gen/MemItem").MemItem, "id" | "kind" | "scope" | "title" | "summary" | "area" | "tags" | "body" | "source" | "importance" | "created_at" | "updated_at" | "accessed_at"> & {author?: import("../../../gen/MemItem").MemItem["author"] | null};

export type NodeKind = "basic" | "expanded" | "image";

export interface Viewport {
  panX: number;
  panY: number;
  scale: number;
}
