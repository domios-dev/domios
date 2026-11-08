import { spawnAiHelper, withAiHelperContext, type AiHelperSpawned } from "./aiHelperSpawn";

/**
 * What the General agent is asked when the user wants an ACP agent. With a subject (a profile or what the user
 * typed) it connects that agent; without one it first explains ACP in plain words.
 */
export function acpSetupRequest(subject?: string): string {
  const named = subject?.trim();
  return [
    named ? `Connect an ACP agent: ${named}.` : "Explain in one or two plain sentences what ACP is in Domios, then help me connect an ACP agent.",
    "Use `domios agent acp --help`: list the profiles, search the registry, install with my consent, sign it in, start a short test session and tell me how it went.",
    "Reply in my language.",
  ].join(" ");
}

export function startAcpSetup(subject?: string): Promise<AiHelperSpawned> {
  return spawnAiHelper({ title: "ACP", description: "Connect an ACP agent", task: withAiHelperContext(acpSetupRequest(subject)) });
}
