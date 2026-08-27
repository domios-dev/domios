/** Provider-declared in-agent model switch template; `{model}` is a ModelInfo id. */
export function formatModelSwitch(format: string, modelId: string): string {
  return format.includes("{model}")
    ? format.replaceAll("{model}", modelId)
    : `${format} ${modelId}`;
}
