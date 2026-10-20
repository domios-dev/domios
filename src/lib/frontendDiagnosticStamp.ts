export interface DeviceDiagnosticInput {
  appVersion?: string; userAgent: string; width: number; height: number; visualWidth: number; visualHeight: number;
  pixelRatio: number; touch: boolean; standalone: boolean; scrollEnd: boolean; overflowAnchor: boolean;
}

export function diagnosticBuildSha(value: string): string | undefined {
  return /^[a-zA-Z0-9_.-]{1,64}$/.test(value) ? value : undefined;
}

export function frontendDiagnosticStamp(buildSha: string, entryUrl: string, input: DeviceDiagnosticInput) {
  const entryAsset = entryUrl.split(/[?#]/)[0].split("/").pop() ?? "";
  const dimension = (value: number) => Number.isFinite(value) ? Math.round(Math.max(0, Math.min(32768, value))) : 0;
  const ua = input.userAgent;
  const userAgentFamily = /Edg(?:e|A|iOS)?\//.test(ua) ? "edge" : /(?:Firefox|FxiOS)\//.test(ua) ? "firefox"
    : /(?:Chrome|CriOS)\//.test(ua) ? "chrome" : /Safari\//.test(ua) ? "safari" : "other";
  return {
    buildSha: diagnosticBuildSha(buildSha),
    device: {
      ...(diagnosticBuildSha(input.appVersion ?? "") ? { appVersion: input.appVersion } : {}),
      entryAsset: /^[a-zA-Z0-9_.-]{1,100}$/.test(entryAsset) ? entryAsset : undefined,
      userAgentFamily, width: dimension(input.width), height: dimension(input.height),
      visualWidth: dimension(input.visualWidth), visualHeight: dimension(input.visualHeight),
      pixelRatio: Number.isFinite(input.pixelRatio) ? Math.max(0, Math.min(16, input.pixelRatio)) : 0,
      touch: input.touch, displayMode: input.standalone ? "standalone" : "browser",
      scrollEnd: input.scrollEnd, overflowAnchor: input.overflowAnchor,
    },
  };
}
export type FrontendDeviceDiagnostic = ReturnType<typeof frontendDiagnosticStamp>["device"];
