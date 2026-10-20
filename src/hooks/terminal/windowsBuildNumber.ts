import { nativeBridge } from "../../lib/nativeBridge";

/** ConPTY reflow defaults to the Windows 10 2004 build until the host reports its own. */
export const DEFAULT_WINDOWS_BUILD_NUMBER = 19041;

let known: number | undefined;
let pending: Promise<number> | null = null;

export function knownWindowsBuildNumber(): number {
  return known ?? DEFAULT_WINDOWS_BUILD_NUMBER;
}

/** The host build number is read once per app; every terminal mount after that reuses it. */
export function readWindowsBuildNumber(read: () => Promise<number> = () => nativeBridge.windowsBuildNumber()): Promise<number> {
  pending ??= read().then((value) => (known = value), () => DEFAULT_WINDOWS_BUILD_NUMBER);
  return pending;
}

export function resetWindowsBuildNumberForTests(): void {
  known = undefined;
  pending = null;
}
