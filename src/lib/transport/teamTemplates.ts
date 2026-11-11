import type { TransportClient } from "../transportClient";

export type TemplateList = import("../../../gen/TemplateList").TemplateList;
export type TemplatePreview = import("../../../gen/TemplatePreview").TemplatePreview;
export type TemplateRecall = import("../../../gen/TemplateRecall").TemplateRecall;
export type TemplateRef = import("../../../gen/TemplateRef").TemplateRef;
export type TemplateStartRequest = import("../../../gen/TemplateStartRequest").TemplateStartRequest;
export type TemplateStartResult = import("../../../gen/TemplateStartResult").TemplateStartResult;
export type TemplateUnavailable = import("../../../gen/TemplateUnavailable").TemplateUnavailable;

export type TeamTemplatesClient = Pick<TransportClient, "json">;

/** Team templates over the shared REST contracts; starting goes through the ordinary pair request with `template`. */
export function createTeamTemplatesTransport(client: TeamTemplatesClient) {
  return {
    list: (opts?: { signal?: AbortSignal }) => client.json<TemplateList>("GET", "/api/team-templates", undefined, opts),
    preview: (template: TemplateRef, opts?: { signal?: AbortSignal }) =>
      client.json<TemplatePreview>("POST", "/api/team-templates/preview", { template }, opts),
    recall: (query: string, opts?: { signal?: AbortSignal }) =>
      client.json<TemplateRecall>("POST", "/api/team-templates/recall", { query }, opts),
  };
}

/** The pair request body for a template start; omitting `template` keeps today's default pair. */
export function withTemplate<T extends Record<string, unknown>>(params: T, template?: TemplateStartRequest): T & { template?: TemplateStartRequest } {
  return template ? { ...params, template } : params;
}
