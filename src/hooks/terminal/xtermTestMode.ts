export interface XtermTestModeOptions {
  isViteDev: boolean;
  viteCodeTermTest?: string | boolean;
  locationSearch?: string;
  locationProtocol?: string;
  locationHostname?: string;
  locationPort?: string;
}

export function shouldEnableXtermA11yMode({
  isViteDev,
  viteCodeTermTest,
  locationSearch,
  locationProtocol,
  locationHostname,
  locationPort,
}: XtermTestModeOptions): boolean {
  const localDevRuntime =
    locationProtocol === "http:" &&
    locationPort !== "7685" &&
    (locationHostname === "127.0.0.1" || locationHostname === "localhost");
  if (!isViteDev && !localDevRuntime) return false;
  const harnessRequested = viteCodeTermTest === "1" || viteCodeTermTest === true;
  if (harnessRequested) return true;

  if (!locationSearch) return false;
  const params = new URLSearchParams(locationSearch);
  return params.has("a11y") || params.has("e2e");
}
