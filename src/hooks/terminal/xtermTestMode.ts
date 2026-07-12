export interface XtermTestModeOptions {
  isViteDev: boolean;
  viteCodeTermTest?: string | boolean;
  locationSearch?: string;
}

export function shouldEnableXtermA11yMode({
  isViteDev,
  viteCodeTermTest,
  locationSearch,
}: XtermTestModeOptions): boolean {
  const harnessRequested = viteCodeTermTest === "1" || viteCodeTermTest === true;
  if (isViteDev && harnessRequested) return true;

  if (!locationSearch) return false;
  const params = new URLSearchParams(locationSearch);
  return params.has("a11y") || params.has("e2e");
}
