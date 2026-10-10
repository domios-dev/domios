export type LocalArtifactKind = "page" | "image" | "document";
export interface LocalArtifactDirective { kind: LocalArtifactKind; path: string; }
export function parseLocalArtifactDirective(source: string): LocalArtifactDirective | null {
  const match = /^\s*@ds-(page|image|document)\s+(?:"([^"]+)"|'([^']+)'|(.+?))\s*$/i.exec(
    source.replace(/\r/g, ""),
  );
  const path = (match?.[2] || match?.[3] || match?.[4] || "").trim();
  if (!match || !path || path.includes("\n")) return null;
  return { kind: match[1].toLowerCase() as LocalArtifactKind, path };
}

