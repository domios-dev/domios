import type { LocalArtifactKind } from "@codeterm/chat-core";
export type { LocalArtifactKind } from "@codeterm/chat-core";

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

let opener: ((path: string) => Promise<unknown>) | null = null;
export function configureLocalArtifactOpener(next: typeof opener) { opener = next; }
export async function openLocalArtifact(path: string) {
  if (!opener) throw new Error("Opening local files is unavailable");
  await opener(path);
}
