export const AGENT_WORKING_LINE_SEP = " · ";

export function formatAgentWorkingLine(parts: {
  providerName: string | null | undefined;
  model: string | null | undefined;
  activityText: string | null | undefined;
}): string | null {
  const activity = parts.activityText?.trim();
  if (!activity) return null;
  const segments: string[] = [];
  const provider = parts.providerName?.trim();
  const model = parts.model?.trim();
  if (provider) segments.push(provider);
  if (model) segments.push(model);
  segments.push(activity);
  return segments.join(AGENT_WORKING_LINE_SEP);
}
