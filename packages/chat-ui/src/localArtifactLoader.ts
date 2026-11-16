import type { LocalArtifactKind } from "@codeterm/chat-core";
export type { LocalArtifactKind } from "@codeterm/chat-core";

export type LocalPreviewFile = import("../../../gen/LocalPreviewFile").LocalPreviewFile;

/** `canvas` names the `<ds-canvas src>` file that lists `path` as an item. */
export type LocalArtifactLoader = (
  path: string,
  kind: LocalArtifactKind,
  tabId?: string,
  canvas?: string,
) => Promise<LocalPreviewFile>;

let loader: LocalArtifactLoader | null = null;

export function configureLocalArtifactLoader(next: LocalArtifactLoader | null): void {
  loader = next;
}

export async function loadLocalArtifact(
  path: string,
  kind: LocalArtifactKind,
  tabId?: string,
  canvas?: string,
): Promise<LocalPreviewFile> {
  if (!loader) throw new Error("Local artifact previews are unavailable");
  return canvas === undefined ? loader(path, kind, tabId) : loader(path, kind, tabId, canvas);
}

let opener: ((path: string) => Promise<unknown>) | null = null;
export function configureLocalArtifactOpener(next: typeof opener) { opener = next; }
export async function openLocalArtifact(path: string) {
  if (!opener) throw new Error("Opening local files is unavailable");
  await opener(path);
}

export function localArtifactOpeningAvailable(): boolean { return opener !== null; }
