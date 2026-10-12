export type LocalArtifactKind = "page" | "image" | "document";
export interface LocalArtifactDirective { kind: LocalArtifactKind; path: string; }
export function parseLocalArtifactDirective(source: string): LocalArtifactDirective | null {
  // `@ct-` is the pre-Domios prefix that older prompts and peers still emit.
  const match = /^\s*@(?:ds|ct)-(page|image|document)\s+(?:"([^"]+)"|'([^']+)'|(.+?))\s*$/i.exec(
    source.replace(/\r/g, ""),
  );
  const path = (match?.[2] || match?.[3] || match?.[4] || "").trim();
  if (!match || !path || path.includes("\n")) return null;
  return { kind: match[1].toLowerCase() as LocalArtifactKind, path };
}


export type LocalArtifactTextPart = { text: string; directive?: LocalArtifactDirective };

export function localArtifactTextParts(source: string): LocalArtifactTextPart[] {
  const parts: LocalArtifactTextPart[] = [];
  let prose = "";
  let fence: string | null = null;
  for (const line of source.split(/(?<=\n)/)) {
    const marker = /^[\t ]*(`{3,}|~{3,})/.exec(line)?.[1];
    if (marker && fence === null) fence = marker;
    else if (marker && fence && marker[0] === fence[0] && marker.length >= fence.length) fence = null;
    const directive = !marker && fence === null && !/^(?: {4}|\t)/.test(line)
      ? parseLocalArtifactDirective(line.replace(/\r?\n$/, "")) : null;
    if (directive) {
      if (prose) parts.push({ text: prose });
      prose = "";
      parts.push({ text: line, directive });
    } else prose += line;
  }
  if (prose) parts.push({ text: prose });
  return parts;
}
