import { openNewSessionPicker } from "../newSessionChoice";

type TemplateRef = import("../../../gen/TemplateRef").TemplateRef;
type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;

/** A saved template to preview, or a package file the dialog imports first. */
export type PendingTemplatePreview =
  | { template: TemplateRef; update: TemplateUpdateDiff | null }
  | { file: string };

type Workspace = { id: string; cwd: string | null };

let pending: PendingTemplatePreview | null = null;

function openWith(workspace: Workspace, request: PendingTemplatePreview): void {
  pending = request;
  openNewSessionPicker({ workspaceId: workspace.id, workspaceCwd: workspace.cwd, section: "workspace" });
}

/** Opens the session dialog on a package's preview; the dialog takes the request once when it mounts. */
export function openTemplatePreview(workspace: Workspace, template: TemplateRef, update: TemplateUpdateDiff | null = null): void {
  openWith(workspace, { template, update });
}

/** Opens the session dialog on a package file the OS handed Domios; it imports it and shows its review, never starts it. */
export function openTemplateFileReview(workspace: Workspace, file: string): void {
  openWith(workspace, { file });
}

export function hasPendingTemplatePreview(): boolean {
  return pending !== null;
}

export function takePendingTemplatePreview(): PendingTemplatePreview | null {
  const request = pending;
  pending = null;
  return request;
}
