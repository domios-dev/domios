import { ioJson, ioEncoded, type HostIoSteps } from "@codeterm/plugin-sdk";
import type { Host } from "@codeterm/plugin-sdk";
import { CLOUD_SIGN_IN_REASON, CODEX_CLOUD_ACCOUNT_REF, cloudEnvironmentConfigList, cloudEnvironmentList, type CloudTaskTransport } from "./backend";
import type { RemoteTaskDiscovery } from "../../../../packages/plugin-sdk/src/remoteTasks";

const CODEX_AUTH_CREDENTIAL = "codexAuth";
const CODEX_ACCOUNT_HEADER = "ChatGPT-Account-Id";
const CLOUD_BASE_URL = "https://chatgpt.com/backend-api";
const SETTINGS_BASE_URL = "https://chatgpt.com";
const SETTINGS_ENVIRONMENTS_PATH = "/api/codex-cloud/v1/environment-configs?scope=user&limit=100&omitDraft=true";
const REPOSITORY_ENVIRONMENTS_PATH = "/wham/environments/by-repo/github/rollacode/codeterm";
const ENVIRONMENTS_PATH = "/wham/environments";
const REQUEST_FAILURE = "Codex Cloud request failed.";

type Row = Record<string, unknown>;

class CloudTransportError extends Error {
  constructor(readonly status?: number) {
    super(status === 401 || status === 403 ? CLOUD_SIGN_IN_REASON : REQUEST_FAILURE);
  }
}

function record(value: unknown): Row | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Row : null;
}

function* publicAccountId(): HostIoSteps<string> {
  let credential: Row | null = null;
  try {
    const raw = (yield* ioJson({ op: "credentialPublic", id: CODEX_AUTH_CREDENTIAL }, true));
    credential = raw ? record(JSON.parse(raw)) : null;
  } catch {
    credential = null;
  }
  const accountId = credential?.accountId;
  if (typeof accountId !== "string" || !accountId) throw new CloudTransportError(401);
  return accountId;
}

function readBody(response: Row): unknown {
  if (typeof response.body !== "string") throw new CloudTransportError();
  try {
    return JSON.parse(response.body) as unknown;
  } catch {
    throw new CloudTransportError();
  }
}

function isCodexTaskPath(method: "GET" | "POST", path: string): boolean {
  if (method === "POST") return path === "/api/codex/tasks";
  return path === "/api/codex/tasks/list"
    || /^\/api\/codex\/tasks\/list\?[^#]*$/.test(path)
    || /^\/api\/codex\/tasks\/[A-Za-z0-9._-]+$/.test(path);
}

export function createCodexCloudTransport(host: Host): CloudTaskTransport {
  const request = function*(method: "GET" | "POST", path: string, accountRef: string, body?: unknown): HostIoSteps<unknown> {
    const settingsRequest = method === "GET" && path === SETTINGS_ENVIRONMENTS_PATH;
    const legacyRequest = path.startsWith("/wham/");
    const codexTaskRequest = isCodexTaskPath(method, path);
    if (accountRef !== CODEX_CLOUD_ACCOUNT_REF || (!settingsRequest && !legacyRequest && !codexTaskRequest)) throw new CloudTransportError();
    const accountId = (yield* publicAccountId());
    let result: unknown;
    try {
      result = JSON.parse((yield* ioEncoded(host.io.fetch({
        url: `${settingsRequest || codexTaskRequest ? SETTINGS_BASE_URL : CLOUD_BASE_URL}${path}`,
        method,
        timeoutMs: 6000,
        headers: {
          [CODEX_ACCOUNT_HEADER]: accountId,
          "Content-Type": "application/json",
          "User-Agent": "codeterm-codex-cloud/1.14.47",
        },
        ...(body === undefined ? {} : {body: JSON.stringify(body)}),
        credential: {
          id: CODEX_AUTH_CREDENTIAL,
          headers: {Authorization: "Bearer {accessToken}"},
        },
      })))) as unknown;
    } catch {
      throw new CloudTransportError();
    }
    const response = record(result);
    if (!response || typeof response.status !== "number") throw new CloudTransportError();
    const status = response.status;
    if (status < 200 || status >= 300) throw new CloudTransportError(status);
    return readBody(response);
  };

  const discoverRemoteTask = function*(): HostIoSteps<RemoteTaskDiscovery> {
    try {
      try {
        const settingsEnvironments = cloudEnvironmentConfigList((yield* request("GET", SETTINGS_ENVIRONMENTS_PATH, CODEX_CLOUD_ACCOUNT_REF)));
        if (settingsEnvironments.length > 0) return {availability: {status: "available"}, environments: settingsEnvironments};
      } catch (error) {
        if (error instanceof CloudTransportError && (error.status === 401 || error.status === 403)) throw error;
      }

      let repoEnvironments: ReturnType<typeof cloudEnvironmentList> | null = null;
      try {
        repoEnvironments = cloudEnvironmentList((yield* request("GET", REPOSITORY_ENVIRONMENTS_PATH, CODEX_CLOUD_ACCOUNT_REF)));
      } catch (error) {
        if (error instanceof CloudTransportError && (error.status === 401 || error.status === 403)) throw error;
      }
      let allEnvironments: ReturnType<typeof cloudEnvironmentList> | null = null;
      try {
        allEnvironments = cloudEnvironmentList((yield* request("GET", ENVIRONMENTS_PATH, CODEX_CLOUD_ACCOUNT_REF)));
      } catch (error) {
        if (error instanceof CloudTransportError && (error.status === 401 || error.status === 403)) throw error;
        if (repoEnvironments === null) throw error;
      }
      const environmentsById = new Map((repoEnvironments ?? []).map(environment => [environment.id, environment]));
      for (const environment of allEnvironments ?? []) {
        if (!environmentsById.has(environment.id)) environmentsById.set(environment.id, environment);
      }
      const environments = [...environmentsById.values()];
      return {availability: {status: "available"}, environments};
    } catch (error) {
      const reason = error instanceof CloudTransportError && (error.status === 401 || error.status === 403)
        ? CLOUD_SIGN_IN_REASON
        : "Could not load Codex Cloud environments. Try again.";
      return {availability: {status: "unavailable", reason}, environments: []};
    }
  };

  return {request, discoverRemoteTask};
}
