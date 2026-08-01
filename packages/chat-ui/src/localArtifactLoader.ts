export type LocalArtifactKind = "page" | "image" | "document";

export interface LocalPreviewFile {
  data: string;
  media_type: string;
}

export type LocalArtifactLoader = (
  path: string,
  kind: LocalArtifactKind,
) => Promise<LocalPreviewFile>;

let loader: LocalArtifactLoader | null = null;

export function configureLocalArtifactLoader(next: LocalArtifactLoader | null): void {
  loader = next;
}

export async function loadLocalArtifact(
  path: string,
  kind: LocalArtifactKind,
): Promise<LocalPreviewFile> {
  if (!loader) throw new Error("Local artifact previews are unavailable");
  return loader(path, kind);
}
