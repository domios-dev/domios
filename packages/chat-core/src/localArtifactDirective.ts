import { parseDsBlocks, type DsBlock } from "./dsBlocks";
export type LocalArtifactKind = "page" | "image" | "video" | "document";
export interface LocalArtifactDirective { kind: LocalArtifactKind; path: string; paths?: string[] }
export function localArtifactFromBlock(block: DsBlock): LocalArtifactDirective | null {
  if (block.kind === "image") return { kind: block.kind, path: block.sources[0], ...(block.sources.length > 1 ? { paths: block.sources } : {}) };
  return block.kind === "page" || block.kind === "document" ? { kind: block.kind, path: block.src } : null;
}
export function parseLocalArtifactDirective(source: string): LocalArtifactDirective | null {
  const segments = parseDsBlocks(source).filter((segment) => segment.type !== "markdown" || segment.text.trim());
  return segments.length === 1 && segments[0].type === "block" ? localArtifactFromBlock(segments[0].block) : null;
}
export type LocalArtifactTextPart = { text: string; directive?: LocalArtifactDirective };
export function localArtifactTextParts(source: string): LocalArtifactTextPart[] {
  return parseDsBlocks(source).map((segment) => {
    const directive = segment.type === "block" ? localArtifactFromBlock(segment.block) : null;
    return { text: source.slice(segment.start, segment.end), ...(directive ? { directive } : {}) };
  });
}
