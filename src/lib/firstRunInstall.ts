import { listen } from "./bridge";
import { launchProviderInstaller } from "./providerInstall";

export interface InstallCompletion {
  provider: string;
  operation: string;
  tab_id: string;
  success: boolean;
  summary: string;
}

export async function installAndWait(providerId: string, signal: AbortSignal): Promise<void> {
  const observed = new Map<string, InstallCompletion>();
  let target: string | undefined;
  let accept: (event: InstallCompletion) => void = () => {};
  const completed = new Promise<InstallCompletion>((resolve) => { accept = resolve; });
  const stop = await listen<InstallCompletion>("provider-cli-completed", ({ payload }) => {
    if (payload.provider !== providerId || payload.operation !== "install") return;
    observed.set(payload.tab_id, payload);
    if (payload.tab_id === target) accept(payload);
  });
  const fail = (summary: string) => accept({ provider: providerId, operation: "install", tab_id: target ?? "", success: false, summary });
  const abort = () => fail("Installation canceled");
  signal.addEventListener("abort", abort, { once: true });
  const deadline = setTimeout(() => fail("Installation timed out. Retry to install again."), 660_000);
  try {
    if (signal.aborted) throw new Error("Installation canceled");
    const launch = await launchProviderInstaller(providerId);
    target = launch.tab_id;
    const early = observed.get(target);
    const result = early ?? await completed;
    if (!result.success) throw new Error(result.summary);
  } finally {
    stop();
    clearTimeout(deadline);
    signal.removeEventListener("abort", abort);
  }
}
