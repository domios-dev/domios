export type LocalArtifactKind = "page" | "image" | "video" | "document";
export interface LocalArtifactDirective { kind: LocalArtifactKind; path: string; paths?: string[]; }
export function parseLocalArtifactDirective(source: string): LocalArtifactDirective | null {
  const match = /^\s*@ds-(page|image|document)\s+(.+?)\s*$/i.exec(source.replace(/\r/g, ""));
  if (!match || match[2].includes("\n")) return null;
  const kind = match[1].toLowerCase() as LocalArtifactKind;
  const argument = match[2];
  const tokens = argument.match(/"[^"]+"|'[^']+'|\S+/g) ?? [];
  const paths = tokens.map(token => token.replace(/^(["'])(.*)\1$/, "$2"));
  const absolute = (path: string) => /^(?:[a-z]:[\\/]|\/|\\\\)/i.test(path);
  if (kind === "image" && paths.length > 1 && paths.every(absolute)) {
    return { kind, path: paths[0], paths };
  }
  const path = argument.replace(/^(["'])(.*)\1$/, "$2").trim();
  if (!path) return null;
  return { kind, path };

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
