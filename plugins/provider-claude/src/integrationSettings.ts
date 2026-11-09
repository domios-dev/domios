import type { HostIoSteps } from "@codeterm/plugin-sdk";
import { INTEGRATION_HOOK_KEYS, type ClaudeSettings } from "./hookOwnership";

export function* readIntegrationSettings(path: string): HostIoSteps<
  { ok: true; settings: ClaudeSettings } | { ok: false; error: string }
> {
  const original = yield* host.io.fs.readFile(path);
  if (typeof original !== "string") return { ok: false, error: "could not safely read settings JSON at " + path };
  let parsed: unknown;
  try {
    parsed = JSON.parse(original);
  } catch (_) {
    return { ok: false, error: "existing settings JSON is malformed at " + path };
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { ok: false, error: "existing settings JSON must contain an object at " + path };
  }
  const settings = parsed as ClaudeSettings;
  if (settings.hooks !== undefined) {
    if (!settings.hooks || typeof settings.hooks !== "object" || Array.isArray(settings.hooks)) {
      return { ok: false, error: "hooks is not an object" };
    }
    for (const key of INTEGRATION_HOOK_KEYS) {
      if (settings.hooks[key] !== undefined && !Array.isArray(settings.hooks[key])) {
        return { ok: false, error: "hooks." + key + " is not an array" };
      }
    }
  }
  return { ok: true, settings };
}
