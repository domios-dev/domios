import type {AgentEvent} from "@codeterm/plugin-sdk";
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
  return windows.length || resets ? [{kind:"rateLimits",rateLimits:windows,...(resets ? {resets} : {})}] : [];
}
