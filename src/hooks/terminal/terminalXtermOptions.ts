export interface TerminalXtermOptions {
  windowsPty?: {
    backend: "conpty";
    buildNumber: number;
  };
  reflowCursorLine: boolean;
}

export function terminalXtermOptions(
  viewerLocalGeometry: boolean,
  windowsBuildNumber: number,
): TerminalXtermOptions {
  return viewerLocalGeometry
    ? { reflowCursorLine: true }
    : {
        windowsPty: { backend: "conpty", buildNumber: windowsBuildNumber },
        reflowCursorLine: false,
      };
}
