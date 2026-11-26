import type { AgentEvent, ProviderSessionObservations } from "@codeterm/plugin-sdk";
import { isFreeZenModel } from "./freeModel";
import { object, text, utf8Length } from "./runtimeProtocol";

interface Message {
  role?: string;
  type?: string;
  providerID?: string;
  modelID?: string;
  model?: { providerID?: string; id?: string };
  error?: TurnError;
}

interface TurnError {
  name?: string;
  type?: string;
  code?: string;
  message?: string;
  status?: number;
  statusCode?: number;
  providerID?: string;
  body?: unknown;
  response?: { body?: unknown };
  data?: {
    message?: string;
    code?: string;
    type?: string;
    status?: number;
    statusCode?: number;
    providerID?: string;
    isRetryable?: boolean;
    body?: unknown;
    responseBody?: unknown;
  };
}

export type OpencodeConsoleKeyFailureKind = "api_key_expired" | "invalid_api_key";

export interface ProviderErrorCommand {
  label: string;
  command: string;
}

export interface ProviderErrorRemediation {
  code: OpencodeConsoleKeyFailureKind;
  title: string;
  message: string;
  commands: ProviderErrorCommand[];
  retry: true;
  rawDetail?: string;
}

const OPENCODE_KEY_COMMANDS: ProviderErrorCommand[] = [
  { label: "Sign in again", command: "opencode auth login" },
  { label: "Remove key to use free models anonymously", command: "opencode auth logout" },
];

const EXPIRED_KEY_CODES = new Set(["api_key_expired", "token_expired", "auth_expired"]);
const INVALID_KEY_CODES = new Set(["invalid_api_key", "invalid_key", "api_key_invalid", "revoked_api_key", "api_key_revoked"]);
const AUTH_ERROR_TYPES = new Set(["provider.auth", "authentication_error", "providerautherror"]);

function clampUtf8(value: string, maxBytes: number): string {
  let bytes = 0;
  let content = "";
  for (const character of value) {
    bytes += utf8Length(character);
    if (bytes > maxBytes) break;
    content += character;
  }
  return content;
}

function parseJsonObject(value: unknown): Record<string, unknown> {
  if (typeof value === "string") {
    if (!value || value.length > 65_536) return {};
    try {
      return object(JSON.parse(value));
    } catch {
      return {};
    }
  }
  return object(value);
}

function finiteStatus(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isInteger(value)) return value;
  if (typeof value === "string" && /^\d{3}$/.test(value)) return Number(value);
  return undefined;
}

function errorBody(error: Record<string, unknown>): Record<string, unknown> {
  const data = object(error.data);
  const response = object(error.response);
  for (const candidate of [response.body, error.body, data.responseBody, data.body]) {
    const parsed = parseJsonObject(candidate);
    if (Object.keys(parsed).length > 0) return parsed;
  }
  return {};
}

function errorStatus(error: Record<string, unknown>, body: Record<string, unknown>): number | undefined {
  const data = object(error.data);
  const nested = object(body.error);
  for (const candidate of [error.status, error.statusCode, data.statusCode, data.status, nested.status, body.status]) {
    const code = finiteStatus(candidate);
    if (code !== undefined) return code;
  }
  return undefined;
}

function providerFromModel(modelId?: string): string | undefined {
  if (!modelId) return undefined;
  const slash = modelId.indexOf("/");
  return slash > 0 ? modelId.slice(0, slash).toLowerCase() : undefined;
}

function messageModelId(value: unknown): string | null {
  const message = value as Message | null;
  if (!message || typeof message !== "object") return null;
  if (message.modelID?.includes("/")) return message.modelID;
  if (message.providerID && message.modelID) return `${message.providerID}/${message.modelID}`;
  const nested = object(message.model);
  const providerID = text(nested.providerID);
  const id = text(nested.id);
  if (id?.includes("/")) return id;
  return providerID && id ? `${providerID}/${id}` : null;
}

export function isQuotaError(value: unknown): boolean {
  const error = object(value);
  const body = errorBody(error);
  return error.type === "provider.quota" ||
    object(body.error).type === "insufficient_balance_error";
}

export function isAuthError(value: unknown): boolean {
  const error = object(value);
  return error.type === "provider.auth" || error.name === "ProviderAuthError";
}

export function runtimeErrorMessage(error: Record<string, unknown>, fallback: string): string {
  const body = errorBody(error);
  return text(object(error.data).message) ?? text(error.message) ?? text(object(body.error).message) ?? text(body.message) ?? fallback;
}

export function classifyOpencodeConsoleKeyError(value: unknown, modelId?: string): ProviderErrorRemediation | null {
  const error = object(value);
  if (!Object.keys(error).length || isQuotaError(error)) return null;
  const data = object(error.data);
  const body = errorBody(error);
  const nested = object(body.error);
  const rawMessage = runtimeErrorMessage(error, "");
  const provider = (text(data.providerID) ?? text(error.providerID) ?? providerFromModel(modelId))?.toLowerCase();
  const mentionsConsole = /opencode\.ai\/console|\bopencode console\b/i.test(rawMessage);
  if (provider && provider !== "opencode") return null;
  if (!provider && !mentionsConsole) return null;

  const status = errorStatus(error, body);
  const codes = [nested.code, body.code, error.code, data.code]
    .filter((entry): entry is string => typeof entry === "string" && entry.length > 0)
    .map((entry) => entry.toLowerCase());
  const types = [nested.type, body.type, error.type, data.type, error.name]
    .filter((entry): entry is string => typeof entry === "string" && entry.length > 0)
    .map((entry) => entry.toLowerCase());
  const authSignal = status === 401 || types.some((entry) => AUTH_ERROR_TYPES.has(entry));

  const expired = codes.some((code) => EXPIRED_KEY_CODES.has(code))
    || (authSignal && /\b(?:api[ _-]?key|key)\b.*\bexpired\b|\bapi_key_expired\b/i.test(rawMessage));
  const invalidExplicit = codes.some((code) => INVALID_KEY_CODES.has(code))
    || (authSignal && /\b(?:invalid|revoked|malformed|rejected)\b.*\b(?:api[ _-]?key|key|token)\b|\b(?:api[ _-]?key|key)\b.*\b(?:invalid|revoked|not valid|rejected)\b/i.test(rawMessage));
  const missingKey = /\b(?:missing|no|provide|set|enter)\b.*\b(?:api[ _-]?key|key)\b|\b(?:api[ _-]?key|key)\b.*\brequired\b/i.test(rawMessage);
  const invalidFreeZen = !expired && !invalidExplicit && !missingKey && authSignal && !!modelId && isFreeZenModel(modelId);

  if (!expired && !invalidExplicit && !invalidFreeZen) return null;
  const code: OpencodeConsoleKeyFailureKind = expired ? "api_key_expired" : "invalid_api_key";
  const title = expired ? "Stored OpenCode Console key has expired" : "Stored OpenCode Console key is invalid";
  const message = expired
    ? "Your stored OpenCode Console key has expired. Sign in again with opencode auth login, or remove the key with opencode auth logout to use free models anonymously."
    : "Your stored OpenCode Console key was rejected by OpenCode Zen. Sign in again with opencode auth login, or remove the key with opencode auth logout to use free models anonymously.";
  const rawDetail = rawMessage ? clampUtf8(rawMessage, 4096) : undefined;
  return {
    code,
    title,
    message,
    commands: OPENCODE_KEY_COMMANDS.map((entry) => ({ ...entry })),
    retry: true,
    ...(rawDetail ? { rawDetail } : {}),
  };
}

export function classifiedTurnError(error: unknown, modelId: string | undefined, fallback: string): { content: string; remediation?: ProviderErrorRemediation } {
  if (isQuotaError(error)) return { content: "Provider balance or quota is exhausted — top up or switch model" };
  const remediation = classifyOpencodeConsoleKeyError(error, modelId);
  if (remediation) return { content: remediation.message, remediation };
  return { content: runtimeErrorMessage(object(error), fallback) };
}

export function runtimeTurnErrorEvents(
  threadId: string,
  turnId: string | undefined,
  seq: number,
  message: string,
  remediation?: ProviderErrorRemediation,
): AgentEvent[] {
  const content = clampUtf8(message, 4096);
  const notice: AgentEvent = { kind: "notice", threadId, message: content };
  if (!turnId) return [notice];
  return [notice, {
    kind: "chatItem", threadId,
    item: {
      id: `${turnId}:error`,
      seq,
      type: "system",
      systemSubtype: "api_error",
      toolFlowId: turnId,
      content,
      ...(remediation ? { toolInput: { remediation } } : {}),
    },
  }];
}

export function turnError(value: unknown): Message["error"] | null {
  if (!value || typeof value !== "object") return null;
  const message = value as Message;
  const error = message.error;
  const assistant = message.role === "assistant" || message.type === "assistant";
  return assistant && error && (error.name === "APIError" || isQuotaError(error) || isAuthError(error)) ? error : null;
}

export function sessionObservations(value: unknown): ProviderSessionObservations | null {
  const error = turnError(value);
  if (!error) {
    const message = value as { role?: string; type?: string; error?: unknown; time?: { completed?: number } } | null;
    const assistant = message?.role === "assistant" || message?.type === "assistant";
    return assistant && !message?.error && typeof message?.time?.completed === "number"
      ? { api_error: null, permission_mode: null, clears_api_error: true } : null;
  }
  const errorRecord = object(error);
  const body = errorBody(errorRecord);
  const status = errorStatus(errorRecord, body);
  const id = messageModelId(value);
  const remediation = classifyOpencodeConsoleKeyError(errorRecord, id ?? undefined);
  const apiError = remediation?.code === "api_key_expired"
    ? "auth_expiry"
    : isQuotaError(errorRecord)
      ? "usage_limit"
      : remediation?.code === "invalid_api_key" || isAuthError(errorRecord) || status === 401 || status === 403
        ? "authentication"
        : status === 429
          ? "rate_limit"
          : "other";
  return { ...(id && unavailableModelError(value) ? { unavailable_model: id } : {}), api_error: apiError, permission_mode: null, clears_api_error: false };
}

export function unavailableModelError(value: unknown): boolean {
  const error = turnError(value);
  return error?.data?.isRetryable === false && (error.data.statusCode === 410
    || /\bmodel\b.*\b(deprecated|unavailable|not found|does not exist|not supported)\b/i.test(error.data.message ?? ""));
}

export function modelMetadata(stdout: string): Map<string, Record<string, unknown>> {
  const metadata = new Map<string, Record<string, unknown>>();
  let id: string | null = null;
  let lines: string[] = [];
  const finish = () => {
    if (!id) return;
    try {
      const value: unknown = JSON.parse(lines.join("\n"));
      if (value && typeof value === "object" && !Array.isArray(value)) metadata.set(id, value as Record<string, unknown>);
    } catch { }
  };
  for (const line of stdout.split("\n")) {
    if (/^[a-z0-9][a-z0-9_.-]*\/[a-z0-9][a-z0-9_.-]*$/i.test(line.trim())) {
      finish(); id = line.trim(); lines = [];
    } else if (id) lines.push(line);
  }
  finish();
  return metadata;
}

export function modelUnavailable(metadata?: Record<string, unknown>): boolean {
  return metadata?.status === "deprecated" || metadata?.status === "unavailable"
    || metadata?.deprecated === true || metadata?.available === false;
}

export function rejectedModelIds(rows: Array<{ model: string | null; message: unknown }>): Set<string> {
  const seen = new Set<string>();
  const rejected = new Set<string>();
  for (const { model, message } of rows) {
    if (!model || seen.has(model)) continue;
    seen.add(model);
    if (unavailableModelError(message)) rejected.add(model);
  }
  return rejected;
}
