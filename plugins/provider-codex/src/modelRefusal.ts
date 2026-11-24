import { object, text } from "./runtimeProtocol";

export type ModelRefusal = { model: string; message: string };

function httpError(message: string | undefined): unknown {
  const body = message?.startsWith("{") ? message
    : message?.match(/(?:^|unexpected status )400 Bad Request:\s*(\{.*\})(?:, url:|$)/s)?.[1];
  if (!body) return null;
  try {
    return JSON.parse(body);
  } catch {
    return null;
  }
}

export function modelRefusal(value: unknown, requestedModel?: string): ModelRefusal | undefined {
  const failure = object(value);
  const diagnostic = text(failure.additionalDetails) ?? text(failure.message);
  const data = object(failure.data);
  const body = [failure, data, object(httpError(diagnostic))]
    .map(candidate => object(candidate.error))
    .find(candidate => candidate.code === "model_not_found" && candidate.type === "invalid_request_error");
  const message = text(body?.message) ?? diagnostic;
  const namedModel = message?.match(/^The '([^'\s]+)' model is not supported when using Codex with a ChatGPT account\.$/)?.[1];
  if (!body && !namedModel) return;
  const model = namedModel ?? text(body?.model) ?? requestedModel;
  if (!model || !message) return;
  return {model, message};
}
