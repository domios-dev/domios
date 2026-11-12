import { openNewSessionPicker } from "../newSessionChoice";

type TemplateRef = import("../../../gen/TemplateRef").TemplateRef;
type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;

export interface PendingTemplatePreview { template: TemplateRef; update: TemplateUpdateDiff | null }

let pending: PendingTemplatePreview | null = null;

/** Opens the session dialog on a package's preview; the dialog takes the request once when it mounts. */
export function openTemplatePreview(workspace: { id: string; cwd: string | null }, template: TemplateRef, update: TemplateUpdateDiff | null = null): void {
  pending = { template, update };
  openNewSessionPicker({ workspaceId: workspace.id, workspaceCwd: workspace.cwd, section: "workspace" });
}

export function hasPendingTemplatePreview(): boolean {
  return pending !== null;
}

export function takePendingTemplatePreview(): PendingTemplatePreview | null {
  const request = pending;
  pending = null;
  return request;
}
