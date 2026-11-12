import type { TransportClient } from "../transportClient";

export type TemplateList = import("../../../gen/TemplateList").TemplateList;
export type TemplatePreview = import("../../../gen/TemplatePreview").TemplatePreview;
export type TemplateRecall = import("../../../gen/TemplateRecall").TemplateRecall;
export type TemplateRef = import("../../../gen/TemplateRef").TemplateRef;
export type TemplateStartRequest = import("../../../gen/TemplateStartRequest").TemplateStartRequest;
export type TemplateStartResult = import("../../../gen/TemplateStartResult").TemplateStartResult;
export type TemplateUnavailable = import("../../../gen/TemplateUnavailable").TemplateUnavailable;
export type TemplateExportRequest = import("../../../gen/TemplateExportRequest").TemplateExportRequest;
export type TemplateExportResult = import("../../../gen/TemplateExportResult").TemplateExportResult;
export type PackageCheckReport = import("../../../gen/PackageCheckReport").PackageCheckReport;
export type TemplateImportResult = import("../../../gen/TemplateImportResult").TemplateImportResult;
export type TemplateDeleteResult = import("../../../gen/TemplateDeleteResult").TemplateDeleteResult;
export type TemplateShareCard = import("../../../gen/TemplateShareCard").TemplateShareCard;
export type TemplateDistillRequest = import("../../../gen/TemplateDistillRequest").TemplateDistillRequest;
export type TemplateDistillResult = import("../../../gen/TemplateDistillResult").TemplateDistillResult;

export type TemplateFileRequest = import("../../../gen/TemplateFileRequest").TemplateFileRequest;
export type TemplateFileText = import("../../../gen/TemplateFileText").TemplateFileText;
export type TemplatePeerStartRequest = import("../../../gen/TemplatePeerStartRequest").TemplatePeerStartRequest;
export type TemplatePeerStartAccepted = import("../../../gen/TemplatePeerStartAccepted").TemplatePeerStartAccepted;

export type TeamTemplatesClient = Pick<TransportClient, "json">;

/** Team templates over the shared REST contracts; starting goes through the ordinary pair request with `template`. */
export function createTeamTemplatesTransport(client: TeamTemplatesClient) {
  return {
    list: (opts?: { signal?: AbortSignal }) => client.json<TemplateList>("GET", "/api/team-templates", undefined, opts),
    preview: (template: TemplateRef, opts?: { signal?: AbortSignal }) =>
      client.json<TemplatePreview>("POST", "/api/team-templates/preview", { template }, opts),
    recall: (query: string, opts?: { signal?: AbortSignal }) =>
      client.json<TemplateRecall>("POST", "/api/team-templates/recall", { query }, opts),
    export: (request: TemplateExportRequest, opts?: { signal?: AbortSignal }) =>
      client.json<TemplateExportResult>("POST", "/api/team-templates/export", request, opts),
    import: (path: string, opts?: { signal?: AbortSignal }) =>
      client.json<TemplateImportResult>("POST", "/api/team-templates/import", { path }, opts),
    delete: (template: TemplateRef, opts?: { signal?: AbortSignal }) =>
      client.json<TemplateDeleteResult>("POST", "/api/team-templates/delete", { template }, opts),
    file: (request: TemplateFileRequest, opts?: { signal?: AbortSignal }) =>
      client.json<TemplateFileText>("POST", "/api/team-templates/file", request, opts),
    peerStart: (request: TemplatePeerStartRequest, opts?: { signal?: AbortSignal }) =>
      client.json<TemplatePeerStartAccepted>("POST", "/api/team-templates/peer-start", request, opts),
    card: (fileId: string, path: string, opts?: { signal?: AbortSignal }) =>
      client.json<TemplateShareCard>("POST", "/api/team-templates/card", { file_id: fileId, path }, opts),
    distill: ({ team_id: teamId, ...request }: TemplateDistillRequest, opts?: { signal?: AbortSignal }) =>
      client.json<TemplateDistillResult>("POST", `/api/teams/${encodeURIComponent(teamId)}/distill`, request, opts),
  };
}

export type TeamTemplatesTransport = ReturnType<typeof createTeamTemplatesTransport>;

/** The pair request body for a template start; omitting `template` keeps today's default pair. */
export function withTemplate<T extends Record<string, unknown>>(params: T, template?: TemplateStartRequest): T & { template?: TemplateStartRequest } {
  return template ? { ...params, template } : params;
}
