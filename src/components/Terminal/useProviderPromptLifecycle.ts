import { useState } from "react";
import type { Tab } from "../../lib/types";
import { advanceProviderPrompt, providerPromptPresentation, type ProviderPromptLifecycle } from "../../lib/providerPromptLifecycle";

export function useProviderPromptLifecycle(tab: Tab | null | undefined) {
  const [previous, setPrevious] = useState<ProviderPromptLifecycle | null>(null);
  const [answeredKey, setAnsweredKey] = useState<string | null>(null);
  const lifecycle = advanceProviderPrompt(previous, tab, answeredKey);
  if (JSON.stringify(previous) !== JSON.stringify(lifecycle)) setPrevious(lifecycle);
  return { tab: providerPromptPresentation(tab, lifecycle), resolved: lifecycle.resolved,
    key: lifecycle.key, resolve: setAnsweredKey };
}
