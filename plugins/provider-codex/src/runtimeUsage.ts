import type {AgentEvent, RuntimeAccountCredits} from "@codeterm/plugin-sdk";
import {resetCredits} from "./resetCredits";
import {object} from "./runtimeProtocol";

export function rateLimitEvents(raw:unknown, rawResets?:unknown): AgentEvent[] {
  const limits = object(raw);
  if (limits.limitId && limits.limitId !== "codex") return [];
  const windows = ["primary","secondary"].flatMap(window => {
    const value = object(limits[window]);
    if (typeof value.usedPercent !== "number" || !Number.isFinite(value.usedPercent)) return [];
    return [{window,usedPercent:Math.max(0,Math.min(100,value.usedPercent)),
      ...(typeof value.resetsAt === "number" && value.resetsAt >= 0 ? {resetsAt:value.resetsAt * 1000} : {}),
      ...(typeof value.windowDurationMins === "number" && value.windowDurationMins > 0 ? {windowMinutes:value.windowDurationMins} : {})}];
  });
  const resets = resetCredits(rawResets);
  const accountCredits = parseAccountCredits(limits.credits);
  return windows.length || resets || accountCredits ? [{kind:"rateLimits",rateLimits:windows,...(resets ? {resets} : {}),...(accountCredits ? {accountCredits} : {})}] : [];
}

function parseAccountCredits(raw:unknown): RuntimeAccountCredits | undefined {
  const value = object(raw);
  if (typeof value.hasCredits !== "boolean" || typeof value.unlimited !== "boolean") return undefined;
  if (value.unlimited) return {kind:"unlimited"};
  return typeof value.balance === "string" && /^\d+(?:\.\d+)?$/.test(value.balance) ? {kind:"balance",balance:value.balance} : undefined;
}
