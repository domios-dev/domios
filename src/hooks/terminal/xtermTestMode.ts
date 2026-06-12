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
  return new URLSearchParams(locationSearch).has("a11y");
}
