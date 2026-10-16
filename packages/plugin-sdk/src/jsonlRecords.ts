import type { JsonValue } from "./agentRuntime";

export interface ParsedSessionRecord {
  value: JsonValue;
  offset: number;
  row: number;
  oversized?: boolean;
  bytes?: number;
}

export interface JsonlRecordFilter {
  type?: { path: string[]; in: string[] };
  where?: {
    path: string[];
    equals: JsonValue | { placeholder: "sessionId" };
  }[];
}

export interface JsonlRecordSource {
  format: "jsonl";
  filter: JsonlRecordFilter;
  maxRecordBytes?: number;
  oversized?: { keep: string[][] };
}

/** Manifest capabilities.recordSources selects the matching parsed-record hook. */
export type JsonlRecordSources = Record<string, JsonlRecordSource>;

export interface SessionRecordsMeta {
  fromOffset: number;
  toOffset: number;
  newOffset: number;
  truncatedTail: boolean;
  counts: { total: number; matched: number; invalid: number; oversized: number };
}
