import { useEffect, useState } from "react";
import { type AppearanceLook, type ContrastCheck } from "../../lib/appearance";
import { appearanceError, checkAppearanceContrast, type AppearanceTarget } from "../../lib/appearanceClient";

const NO_FAILURES: ContrastCheck[] = [];

export function useAppearanceContrast(target: AppearanceTarget, draft: AppearanceLook | undefined, enabled: boolean) {
  const [result, setResult] = useState<{ target: AppearanceTarget; look: AppearanceLook; failures: ContrastCheck[] }>();
  const [failure, setFailure] = useState<{ look: AppearanceLook; message: string }>();
  const [attempt, retry] = useState(0);

  useEffect(() => {
    if (!draft || !enabled) return;
    const controller = new AbortController();
    void Promise.resolve().then(async () => {
      if (controller.signal.aborted) return;
      try {
        const checks = await checkAppearanceContrast(target, draft, controller.signal);
        if (!controller.signal.aborted) {
          setResult({ target, look: draft, failures: checks.filter(check => check.ratio < check.floor) });
          setFailure(undefined);
        }
      } catch (error) {
        if (!controller.signal.aborted) setFailure({ look: draft, message: appearanceError(error) });
      }
    });
    return () => controller.abort();
  }, [target, draft, enabled, attempt]);

  const last = result?.target === target ? result : undefined;
  return {
    ready: enabled && last !== undefined && last.look === draft,
    failures: last?.failures ?? NO_FAILURES,
    theme: last?.look.theme,
    error: enabled && failure?.look === draft ? failure?.message : undefined,
    retry: () => { setFailure(undefined); retry(value => value + 1); },
  };
}
