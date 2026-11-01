// Compile-time DEV flag from vite.config.mts; release bundles define it false.
export const DEV_BUILD: boolean = typeof __CODETERM_DEV__ !== "undefined" && __CODETERM_DEV__;
