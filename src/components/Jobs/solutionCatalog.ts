import { parse } from "yaml";
import type { BlueprintDocument } from "../../lib/blueprintApi";
import type { PipelineView } from "../Automation/pipelineModel";
import type { SavedSolution, SolutionDocument } from "./blueprintSolutions";

export type SolutionKind = "pipeline" | "watch" | "call";
export const CATALOG_TAGS_HIDDEN = new Set(["blueprint-library"]);

export interface CatalogEntry {
  solution: SavedSolution;
  document: BlueprintDocument | null;
  kind: SolutionKind | null;
  tags: readonly string[];
  error: string | null;
}

export interface CatalogFilter {
  query: string;
  workspaceId: string | null;
  kind: SolutionKind | null;
  tag: string | null;
}

export const NO_CATALOG_FILTER: CatalogFilter = { query: "", workspaceId: null, kind: null, tag: null };

export function parseSolutionDocument(body: string): BlueprintDocument {
  const value: unknown = parse(body);
  if (!value || typeof value !== "object" || !Array.isArray((value as { steps?: unknown }).steps)) throw new Error("The saved solution is not a Blueprint document");
  return value as BlueprintDocument;
}

/** A callable solution has no trigger of its own; it runs when another Blueprint calls it. */
export function solutionKind(document: BlueprintDocument): SolutionKind {
  if (document.kind === "watch") return "watch";
  return document.trigger ? "pipeline" : "call";
}

export function catalogEntry(solution: SavedSolution, stored: SolutionDocument | Error): CatalogEntry {
  if (stored instanceof Error) return { solution, document: null, kind: null, tags: [], error: stored.message };
  try {
    const document = parseSolutionDocument(stored.body);
    return { solution, document, kind: solutionKind(document), tags: stored.tags.filter((tag) => !CATALOG_TAGS_HIDDEN.has(tag)), error: null };
  } catch (error) {
    return { solution, document: null, kind: null, tags: [], error: error instanceof Error ? error.message : String(error) };
  }
}

function searchable(entry: CatalogEntry): string {
  return [entry.solution.when_to_use, ...(entry.solution.intents ?? []), entry.solution.how ?? "", entry.document?.name ?? "", entry.document?.intent ?? "", ...entry.tags].join("\n").toLowerCase();
}

/** Text matches keep their order unless a semantic ranking is present, which then leads. */
export function filterCatalog(entries: readonly CatalogEntry[], filter: CatalogFilter, ranking: readonly string[] | null = null): CatalogEntry[] {
  const words = filter.query.toLowerCase().split(/\s+/).filter(Boolean);
  const ranked = new Map((ranking ?? []).map((id, index) => [id, index]));
  const kept = entries.filter((entry) => (filter.workspaceId === null || entry.solution.workspaceId === filter.workspaceId)
    && (filter.kind === null || entry.kind === filter.kind)
    && (filter.tag === null || entry.tags.includes(filter.tag))
    && (!words.length || ranked.has(entry.solution.id) || words.every((word) => searchable(entry).includes(word))));
  if (!ranking) return kept;
  return [...kept].sort((a, b) => (ranked.get(a.solution.id) ?? Infinity) - (ranked.get(b.solution.id) ?? Infinity));
}

export function catalogTags(entries: readonly CatalogEntry[]): string[] {
  return [...new Set(entries.flatMap((entry) => entry.tags))].sort();
}

export function catalogKinds(entries: readonly CatalogEntry[]): SolutionKind[] {
  return (["pipeline", "watch", "call"] as const).filter((kind) => entries.some((entry) => entry.kind === kind));
}

export function solutionPipeline(entry: CatalogEntry): PipelineView | null {
  if (!entry.document) return null;
  return {
    id: entry.solution.id, name: entry.document.name, state: "saved", owner: { kind: "tab", tab_id: null, team_id: null },
    nodes: [], edges: [], call_graph: entry.document, expanded_calls: [], runId: null, note: entry.document.intent ?? entry.solution.how,
  };
}
